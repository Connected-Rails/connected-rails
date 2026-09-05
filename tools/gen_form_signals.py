#!/usr/bin/env python3
"""Generate the full-size German mechanical signal family for the example mod.

The models are built in metres and follow the DB H/V daytime and night aspects:

* long- and short-arm Hp signals on 6, 8, 10, 12 and 14 m lattice/narrow masts;
* two- and three-aspect Vr signals at 2.76, 4.87 and 5.37 m disc-centre height;
* low and high Sh 0/Sh 1 mechanical stop signals.

Every asset is original procedural work.  Painted steel, galvanised steel,
enamel, glass, rust and concrete use glTF metallic/roughness PBR materials with
shared external colour, ORM and normal maps.  Macro weathering is modelled separately:
paint chips, oxide collars, grime, streaks, worn pivots and faded faces do not
disappear when texture filtering gets coarse.

Run from anywhere with ``python tools/gen_form_signals.py``.  The accompanying
signal-model RON files are generated too so geometry, node names and bindings
cannot drift apart. During an interactive component review, ``--only MODEL``
rebuilds one canonical asset through the same builders and validators; the
unfiltered command remains the complete catalogue/release gate.
"""

from __future__ import annotations

import argparse
import math
import shutil
from pathlib import Path

from gen_signal_parts import Prim, write_gltf


ROOT = Path(__file__).resolve().parents[1]
MODELS = ROOT / "mods" / "example" / "signal_models"
SIGNALS = ROOT / "mods" / "example" / "signals"
SCRIPTS = ROOT / "mods" / "example" / "scripts"

# Full-size dimensions in metres. Values labelled DB drawing/catalogue come
# from the DB Regelzeichnung numbers listed in mods/example/FORMSIGNALE.md.
# Nominal German form-main-signal heights are measured from the lower edge of
# the rail (Schienenunterkante, SU) to the upper blade pivot.  Signal assets are
# anchored at SO by the simulator, so the 60E1 rail profile used by the example
# route has to be removed from that nominal dimension.  Keeping the two datums
# explicit prevents a later refactor from silently making every signal 172 mm
# too tall again.  The 14-m construction is the documented historical special
# height; the later DR installation principles preferred at most 12 m.
HP_PIVOT_HEIGHTS = (6.00, 8.00, 10.00, 12.00, 14.00)
HP_RAIL_HEIGHT = 0.172                 # 60E1: SO down to rail foot / SU
HP_ARM_PIVOT_SPACING = 2.200           # reconstructed Einheitsbauart spacing
HP_ARM = {
    #                  catalogue length  reconstructed width / end disc
    (False, False): (2.220, 0.240, 0.450),  # 1st, long  S 050.07.2
    (False, True):  (1.820, 0.240, 0.450),  # 1st, short S 050.07.2
    (True, False):  (1.920, 0.220, 0.420),  # 2nd, long  S 050.07.2
    (True, True):   (1.520, 0.220, 0.420),  # 2nd, short S 050.07.2
}
# The catalogue value is the complete enamelled blade from its straight inner
# edge to the outside of the round head.  The historical construction text
# explicitly identifies the portion left of the shaft as the linkage pickup
# and partial counterbalance.  It is not merely a short mounting tab.  A
# near-orthogonal preserved upper blade (Berlin Technikmuseum signal group)
# measures about 1.7 rectangular blade depths from its straight inner edge to
# the visible shaft pin.  A rectified high-resolution Niebüll Hp-2 photograph
# independently puts the lower shaft at about 1.8 of its 220-mm blade depths.
# Both resolve to 0.40 m to the nearest centimetre.  Keep these class-C photo
# reconstructions independent from all four published overall lengths.
HP_ARM_ROOT = {False: 0.400, True: 0.400}
# The rectified Niebüll front photograph gives an upper white end field of
# about 60 percent of the red outer disc; its lower blade gives about 61
# percent.  At the reconstructed outer diameters these are 270 and 256 mm;
# unlike the published total lengths they remain class-C photo reconstructions.
HP_ARM_WHITE_DISC_RADIUS = {False: 0.135, True: 0.128}


def hp_arm_stripe_end(lower=False, shortened=False):
    """X coordinate where the straight white field meets the round head.

    S 050.07.2 supplier artwork and orthogonal prototype photographs show a
    closed red annulus around the white end disc.  The straight white field
    therefore stops at the left tangent of the outer disc; it must never run
    through the annulus into the white insert.  Keeping this transition in one
    helper makes that easy to assert for all four blade lengths.
    """
    blade_len, _blade_width, disc_diameter = HP_ARM[(lower, shortened)]
    root = HP_ARM_ROOT[lower]
    disc_radius = disc_diameter * 0.5
    disc_x = blade_len - root - disc_radius
    return disc_x - disc_radius

# Hauptsignal colour-selector reconstruction from Fig. 107.  The selector has
# its own bearing on the lantern slide, below the blade shaft.  Its two Ø170-mm
# glasses run on a compact 320-mm radius and the slotted hook linkage turns it
# about 60 degrees while the blade itself moves only 45 degrees.  Keeping these
# values independent prevents the former rigidly attached, oversized black
# plate from returning during unrelated blade edits.
HP_SELECTOR_AXIS_DROP = 0.200
# The orthogonal Neukölln rear detail gives the most useful lateral datum:
# the complete folded lantern back sits clear of the outer Gittermast chord,
# with the spring/linkage still fitting between lantern and mast.  The former
# 205-mm radius pushed the lantern 115 mm too far inward, so its service lid
# was sliced by the mast in rear-quarter views.  This remains a class-C photo
# reconstruction, but the clearance relation itself is unambiguous.
HP_SELECTOR_RADIUS = 0.320
HP_SELECTOR_SWING_DEGREES = 60.0
HP_SELECTOR_RING_OUTER_RADIUS = 0.108
HP_SELECTOR_RING_INNER_RADIUS = 0.087
# Mast-head cable wheel.  Its axle orientation is not ambiguous: the
# Schmalmast assembly sheet folds the wheel into the mast-face plane and
# orthogonal prototype rear photographs show its circular face.  The axle
# therefore follows Z (front/rear), just like the blade axle, rather than X
# (left/right).  Both mast constructions use the same wheel; only its support
# differs.  The Gittermast carries it inside an open four-angle head, whereas
# the Schmalmast folds it into the notched side of its compact sheet-metal
# roof.  Keeping one wheel size prevents a holder correction from inventing a
# second, unsupported type of hoist.
HP_HEAD_PULLEY_Z = -0.010
HP_HEAD_CABLE_Z = -0.073
HP_HEAD_PULLEY_RADIUS = 0.085
HP_HEAD_PULLEY_DEPTH = 0.090
HP_HEAD_CABLE_RADIUS = 0.0055
HP_HEAD_CABLE_BOTTOM = 0.115
# The two electrical leads visible on the Siemens lantern back are not short
# decorative stubs.  They leave the two lid glands vertically, pass the lower
# lantern on two-arm signals and enter a small terminal at the mast foot.
HP_LANTERN_LEAD_Z = -0.073
HP_LANTERN_LEAD_RADIUS = 0.0050
HP_LANTERN_LEAD_GLAND_OFFSET = 0.039
HP_LANTERN_LEAD_BOTTOM = 0.245
HP_LANTERN_LEAD_KNEE_Y = 0.445
HP_LANTERN_TERMINAL_X = 0.190
HP_LANTERN_TERMINAL_HALF_SPACING = 0.026
# The photographed return spring lies between lantern selector and mast on
# every blade level.  Its old sign put the upper spring on the opposite side
# of the mast and nine disconnected cylinders made it read as a severed rope.
HP_RETURN_SPRING_X = 0.105
HP_RETURN_SPRING_Z = -0.205
HP_RETURN_SPRING_TOP_DROP = 0.205
HP_RETURN_SPRING_BOTTOM_DROP = 0.475
HP_RETURN_SPRING_RADIUS = 0.012
HP_RETURN_SPRING_TURNS = 15.0
HP_RETURN_SPRING_WIRE_RADIUS = 0.0025
# The upper blade holder carries a short Z-offset arm and a laminated,
# hammer-shaped weight.  The supplied rear photographs and the 1:87 assembly
# sequence linked in FORMSIGNALE.md both show this as a compact fitting directly
# on the blade shaft, not the former 830-mm-long chain of two diamond plates.
# Values remain class-C photo reconstructions.
HP_UPPER_HOLDER_BEND = (-0.135, -0.095)
HP_UPPER_HOLDER_WEIGHT_CENTRE = (-0.305, -0.235)
HP_UPPER_HOLDER_WEIGHT_SPAN = 0.160
HP_UPPER_HOLDER_WEIGHT_WIDTH = 0.075
# Separate equalising levers on their shared mid-mast axle.  The construction
# sheet places this bearing at half mast height; rear photographs show the two
# uncoupled hammer levers as a shallow V.  The free drawing has no production
# dimensions, so reach, head size and 18-degree working travel remain class C.
HP_EQUALIZER_HEIGHT_FACTOR = 0.500
HP_EQUALIZER_REACH = 0.320
HP_EQUALIZER_DROP = 0.165
HP_EQUALIZER_WEIGHT_SPAN = 0.145
HP_EQUALIZER_WEIGHT_WIDTH = 0.070
HP_EQUALIZER_SWING_DEGREES = 18.0
# Open 500-mm-stroke end drive reconstructed from historical Figs. 108/109.
# The accessible drawing does not dimension the casing, so these remain class
# C values.  Its topology is explicit: the drive disc sits to the side of the
# mast, has raised running grooves and carries one or two separate angle levers.
HP_END_DRIVE_CENTRE = (0.250, 0.650, -0.250)
HP_END_DRIVE_RADIUS = 0.205
HP_END_DRIVE_DEPTH = 0.064
VR_CENTRE_HEIGHTS = (2.76, 4.87, 5.37)  # historical installation sheets
VR_DISC_DIAMETER = 1.000                 # DB S 090.25.2
VR_DISC_WHITE_RADIUS = 0.490             # photo reconstruction inside Ø1000
VR_DISC_BLACK_RING_RADIUS = 0.435        # photo reconstruction
VR_DISC_FACE_RADIUS = 0.400              # photo reconstruction
NE2_SIZES = {"hoch": (0.480, 0.750),     # DB S 525.1, width x height
             "niedrig": (0.300, 0.450)}
MAST_BOARD_WIDTH = {"gitter": 0.200, "schmal": 0.100}
HP_RED_BOARD_LENGTH = 0.998              # S 055.03.5 / S 370.43.3
HP_WHITE_BOARD_LENGTH = 0.998            # S 055.03 / S 370.43, Hp sequence
HP_SCHMAL_MAST_WIDTH = 0.100             # documented clearance width
HP_SCHMAL_MAST_DEPTH = 0.250             # class-C section reconstruction
HP_SCHMAL_MAST_PLATE = 0.012             # class-C welded plate gauge
# Mast-tip layouts are deliberately construction-specific.  The pulley itself
# is common.  On the Gittermast it lies inside the open top cage; on the
# Schmalmast it is tangent to the 100-mm-wide folded roof.  For the Schmalmast,
# the supplied side-on reference is the least perspective-distorted datum: when
# normalised against the documented 240-mm blade, its guarded wheel axis is
# about 320 mm above the blade axis and the cap about 430 mm above it.
HP_HEAD_LAYOUTS = {
    "gitter": {
        "above_pivot": 0.405,
        "pulley_offset": 0.292,
        "pulley_centre_x": 0.000,
        "support_half_width": 0.145,
    },
    "schmal": {
        "above_pivot": 0.430,
        "pulley_offset": 0.320,
        "pulley_centre_x": 0.135,
        "support_half_width": 0.050,
    },
}
LANTERN_GLASS_DIAMETER = 0.170
VR_LANTERN_CASE = (0.250, 0.500)          # reconstructed around Ø170 mm optics
VR_LED_LANTERN_CASE = (0.220, 0.490)      # moving two-colour filter carrier
VR_LANTERN_APERTURE_SPACING = 0.270
# Orthographic reconstruction from the supplied, nearly frontal 3-aspect
# electric signal photograph, normalized against the documented Ø1000-mm
# disc.  The carrier inner edges sit about 45 mm clear of the 240-mm wing.
# Keeping the lateral offset and both vertical rows as named values prevents a
# housing-shape edit from quietly moving the night sign again.
VR_LANTERN_LATERAL_OFFSET = 0.275
VR_LANTERN_RIGHT_DROP = 0.805
VR_LANTERN_LEFT_DROP = 1.315
VR_WING_LENGTH = 1.410                    # DB S 095073, complete outline
VR_WING_WIDTH = 0.240                     # DB S 095073, complete outline
VR_WING_PIVOT_FROM_TOP = VR_WING_LENGTH * 0.5
# Five paired edge fasteners remain visible on the straight framed portion in
# the high-resolution J35-326 front photograph.  They are small dark/steel
# hardware, not the former orange decorative dots.
VR_WING_EDGE_FASTENERS_FROM_TOP = (0.120, 0.365, 0.610, 0.855, 1.100)
VR_LED_LENS_EDGE_DEPTH = 0.0012
VR_LED_LENS_CROWN = 0.0068
VR_LED_LENS_CENTRE_DEPTH = 0.0082
VR_LED_FRESNEL_STEP = 0.00015
# The folding disc owns this crank in its local coordinate system.  Keeping
# both points explicit lets validation reject the former duplicate crank in
# the static mast mesh—the source of the detached black link in Vr 1.
VR_DISC_CRANK_ROOT = (0.095, -0.020, -0.052)
VR_DISC_CRANK_PIN = (0.315, -0.245, -0.072)
# Vorsignalantrieb, photo/drawing reconstruction.  Fig. 122 does not publish
# dimensions, but it unambiguously makes the upper Seilrad larger than the
# closed Stellscheibe below it.  Keep that visual relation explicit so an
# unrelated detail edit cannot turn the assembly back into two equal boxes or
# a small wire wheel.
VR_DRIVE_STELL_RADIUS = 0.165
VR_DRIVE_SEILRAD_RADIUS = 0.195
# The photographed Vorsignal linkage uses one substantial round pull rod for
# the disc and, only on a three-aspect signal, a second rod for the additional
# wing.  Both sit close to the mast and remain positively connected from the
# drive output to their upper bell cranks.  The old pair at x=355/425 mm were
# only 14 mm in diameter and ended in free space, which made them read as two
# floating wires in the train-facing view.
VR_OPERATING_ROD_RADIUS = 0.012
VR_OPERATING_ROD_MAST_CLEARANCE = 0.025
VR_OPERATING_ROD_REAR_OFFSET = 0.035
# Electric night-sign wiring is a separate, flexible installation.  It joins
# both fixed lantern backs in a small rear junction box and enters the mast at
# the foot; it must never be represented by a detached vertical line.
VR_LIGHTING_CABLE_RADIUS = 0.006
VR_LIGHTING_JUNCTION_X = 0.000
VR_LIGHTING_JUNCTION_DROP = 1.780
VR_LIGHTING_CONDUIT_REAR_OFFSET = 0.035
VR_LIGHTING_JUNCTION_REAR_OFFSET = 0.060
# Siemens electrical signal drive reconstructed from the preserved J35-323 /
# J35-324 Fahrsperrenantrieb and J35-322 release-coupling attachment.  No
# dimensioned works drawing is publicly available, so the envelope remains a
# class-C photo reconstruction.  Keeping the two cases separate is the
# important constructional fact: the upper release-coupling attachment is
# required only by the three-aspect distant signal, while the lower motor and
# locking drive is common to electrically operated variants.
VR_ELECTRIC_DRIVE_LOWER = (0.410, 0.760, 0.340)  # width, height, depth
VR_ELECTRIC_DRIVE_UPPER = (0.380, 0.570, 0.310)
VR_ELECTRIC_DRIVE_X = 0.110
VR_ELECTRIC_DRIVE_LOWER_Y = 0.600
VR_ELECTRIC_DRIVE_UPPER_Y = 1.300
VR_1944_MAST_WIDTH = 0.100             # class-C profile reconstruction
VR_1944_MAST_DEPTH = 0.250
VR_1944_MAST_PLATE = 0.012
VR_OLD_U_MAST_WIDTH = 0.160             # class-C J35/photo reconstruction
VR_OLD_U_MAST_DEPTH = 0.120
VR_OLD_U_MAST_PLATE = 0.010
VR_OLD_U_MAST_GAP = 0.040
SIGNAL_LODS = ((0, 120.0), (1, 450.0), (2, 1600.0))
HP_LODS = SIGNAL_LODS
VR_LODS = SIGNAL_LODS
# Fixed Hp geometry is deliberately split along the real functional assemblies.
# This is more than naming: the signal workbench fingerprints each prefix
# separately, so changing a mast plate can no longer silently reshape the
# lattice, head mechanism, operating rods or end drive.
HP_STATIC_STEMS = (
    "mast_foundation",
    "mast_structure",
    "mast_board",
    "mast_head",
    "mast_rods",
    "mast_drive",
)
# Foundation and enamel sheets are already minimal geometry; duplicating them
# in each distance level keeps the LOD node contract stable, but reducing their
# few planar faces would only damage silhouette or dimensions.
HP_REDUCED_LOD_STEMS = (
    "mast_structure",
    "mast_head",
    "mast_rods",
    "mast_drive",
)
# The available works drawings do not dimension the Hp lattice bays.  Keep
# this class-C pitch separate from the source-backed handedness rule below so
# a later measured drawing can refine spacing without changing topology again.
HP_GITTER_BAY_PITCH = 0.58
SH_CASE_SIZE = 0.700
SH_CASE_DEPTH = 0.400                     # 1938 drawing evaluation
SH_LIT_DISC_DIAMETER = 0.560
SH_HIGH_CENTRE = 4.010                    # 1938 construction drawing
SH_LOW_CENTRE = 0.820                     # reconstructed low support
SH_REAR_MARKER_DIAMETER = 0.100           # scaled from the 1938 drawing
SH_REAR_MARKER_X = 0.160                  # reconstructed marker centres
SH_REAR_MARKER_Y = 0.180


def hp_pivot_above_so(nominal_height):
    """Convert the documented SU datum to the simulator's SO datum."""
    return nominal_height - HP_RAIL_HEIGHT


def hp_pivot_levels(nominal_height):
    """Return both Hp blade axes above SO from one shared height datum.

    The lower blade spacing is a construction dimension between the two axes,
    so it must be subtracted *after* converting the documented upper-axis
    height from SU to the model's SO origin.  Keeping this in one helper stops
    the fixed lantern and moving blade assemblies from drifting apart.
    """
    upper = hp_pivot_above_so(nominal_height)
    return upper, upper - HP_ARM_PIVOT_SPACING


def hp_head_layout(mast):
    """Return the dimensional recipe for one Hp mast head."""
    try:
        return HP_HEAD_LAYOUTS[mast]
    except KeyError as error:
        raise ValueError(
            f"unknown Hauptsignal mast construction: {mast}"
        ) from error


def hp_head_pulley_centre(height, mast):
    """Return the common redirect wheel in its construction-specific holder."""
    layout = hp_head_layout(mast)
    return (layout["pulley_centre_x"],
            height + layout["pulley_offset"], HP_HEAD_PULLEY_Z)


def hp_head_cable_endpoints(height, mast):
    """Return the visible outer leg of the continuous lantern-hoist cable."""
    return hp_head_cable_runs(height, mast)[1]


def hp_head_cable_runs(height, mast):
    """Return both continuous legs from the top wheel into the mast foot.

    The old geometry drew two unrelated short lines: one stopped below the
    mast-head wheel and another pair stopped several decimetres below the
    blade.  A real lantern hoist is a double cable run.  Both legs therefore
    leave the common wheel tangentially, remain in one rear depth plane and
    enter the mast foot instead of ending in free space.
    """
    wheel_x, wheel_y, _wheel_z = hp_head_pulley_centre(height, mast)
    return tuple(
        ((wheel_x + side * HP_HEAD_PULLEY_RADIUS,
          wheel_y, HP_HEAD_CABLE_Z),
         (wheel_x + side * HP_HEAD_PULLEY_RADIUS,
          HP_HEAD_CABLE_BOTTOM, HP_HEAD_CABLE_Z))
        for side in (-1.0, 1.0)
    )


def hp_lantern_lead_paths(nominal_height):
    """Return the two continuous electric leads from lantern to mast foot.

    On a two-arm signal both lantern backs use the same vertical pair: the
    upper leads pass directly through the corresponding lower glands rather
    than spawning four overlapping wires.  Keeping the complete runs in
    ``mast_rods`` also prevents the small ``laterne`` review node from gaining
    a mast-height-sized bounding box.
    """
    upper_y, _lower_y = hp_pivot_levels(nominal_height)
    lamp_x, lamp_dy = hp_lamp_offset(False)
    gland_y = upper_y + lamp_dy - 0.052
    return tuple(
        ((lamp_x + side * HP_LANTERN_LEAD_GLAND_OFFSET,
          gland_y, HP_LANTERN_LEAD_Z),
         (lamp_x + side * HP_LANTERN_LEAD_GLAND_OFFSET,
          HP_LANTERN_LEAD_KNEE_Y, HP_LANTERN_LEAD_Z),
         (HP_LANTERN_TERMINAL_X
          + side * HP_LANTERN_TERMINAL_HALF_SPACING,
          HP_LANTERN_LEAD_BOTTOM, HP_LANTERN_LEAD_Z))
        for side in (-1.0, 1.0)
    )


def hp_end_drive_rod_joints(arms):
    """Return the actual outer joints of the one/two end-drive levers."""
    cx, cy, cz = HP_END_DRIVE_CENTRE
    rear_face = cz - HP_END_DRIVE_DEPTH * 0.5 - 0.020
    joints = [(0.360, cy + 0.385, rear_face)]
    if arms == 2:
        front_face = cz + HP_END_DRIVE_DEPTH * 0.5 + 0.018
        joints.append((0.435, cy + 0.405, front_face))
    return tuple(joints)


def hp_operating_rod_paths(nominal_height, arms):
    """Return connected rear operating-rod polylines for every blade.

    Each path starts in the real end-drive lever joint, runs through its guide
    shoes and finishes in a short dog-leg at the blade-shaft mechanism.  The
    bends are essential: the preserved rear views never show a featureless
    vertical wire hovering beside the mast.
    """
    upper_y, lower_y = hp_pivot_levels(nominal_height)
    levels = [upper_y] if arms == 1 else [upper_y, lower_y]
    paths = []
    for index, (pivot_y, drive_joint) in enumerate(
            zip(levels, hp_end_drive_rod_joints(arms))):
        # The lever itself projects farther sideways than the long vertical
        # run.  A lower crank therefore brings the rod back close to the mast,
        # as in the rear photographs and the assembly schematic.
        rod_x = 0.235 + index * 0.045
        rod_z = drive_joint[2]
        lower_knee = (rod_x, drive_joint[1] + 0.140, rod_z)
        upper_knee = (rod_x, pivot_y - 0.310, rod_z)
        # The shoe terminates at the fixed shaft cheek.  It remains visibly
        # engaged throughout the blade animation instead of opening a gap at
        # one of the two end positions.
        shaft_x = 0.115 + index * 0.018
        shaft_z = -0.300 if index == 0 else -0.225
        shaft_joint = (shaft_x, pivot_y - 0.105, shaft_z)
        paths.append((drive_joint, lower_knee, upper_knee, shaft_joint))
    return tuple(paths)


def hp_static_stems(scheme):
    """Return the fixed Hp component nodes present for one paint scheme."""
    if scheme == "altanstrich":
        # The historical colour zones are painted directly on the steel mast;
        # there is no separate enamel recognition-board assembly.
        return tuple(stem for stem in HP_STATIC_STEMS if stem != "mast_board")
    return HP_STATIC_STEMS


def hp_gitter_brace_direction(bay, face_sign):
    """Return the handedness of one physical Hp lattice-mast face.

    Each face carries a zig-zag of single diagonals.  The opposite face is the
    mirror image, so front/rear and left/right projections show the crossed
    pattern visible in the historic mechanism drawing and oblique photographs.
    Keeping this rule in one tested helper prevents the two depth faces from
    accidentally being emitted on top of each other again.
    """
    if face_sign not in (-1, 1):
        raise ValueError("face_sign must be -1 or +1")
    return (1 if bay % 2 == 0 else -1) * face_sign


def material(name, colour, metallic, roughness, profile, emissive=None,
             texture_size=512, glass=None, clearcoat=None):
    pbr = {
        "baseColorFactor": [*colour, 1.0],
        "metallicFactor": metallic,
        "roughnessFactor": roughness,
        "_texture_profile": profile,
    }
    if texture_size != 128:
        pbr["_texture_size"] = texture_size
    if glass is not None:
        pbr["_glass"] = glass
    if clearcoat is not None:
        pbr["_clearcoat"] = clearcoat
    return (
        name,
        pbr,
        emissive,
    )


# Material names describe function rather than one livery so identical geometry
# can be emitted with historically different paint recipes.
MAT_STRUCTURE = "structure paint"
MAT_GALVANISED = "dull galvanised lamp housing"
MAT_DARK = "matte black ironwork"
MAT_RED = "signal red enamel"
MAT_WHITE = "signal grey-white enamel"
MAT_BLACK = "matte black enamel"
MAT_YELLOW = "warning yellow enamel"
MAT_VR_ORANGE = "Vorsignal orange enamel"
MAT_GAS_BOTTLE = "DB propane bottle pale green"
MAT_RUST = "iron oxide"
MAT_GRIME = "railway grime"
MAT_CONCRETE = "weathered concrete"
MAT_NE2 = "Ne 2 retroreflective face"
MAT_LED_GREEN_FILTER = "LED green filter glass"
MAT_LED_AMBER_FILTER = "LED amber filter glass"
MAT_LED_LIT_GREEN = "lit LED green optical glass"
MAT_LED_LIT_AMBER = "lit LED amber optical glass"

# glTF base-colour factors are linear.  These values are the linearised form of
# the documented display approximations (RAL 6021: 138/153/119, RAL 7011:
# 78/85/88); feeding the sRGB values straight to glTF made both masts much too
# pale in the simulator.
PAINT_SCHEMES = {
    "db_gruen": {
        "label": "DB-Blassgruen RAL 6021",
        # RAL 6021 is the fresh-paint reference.  Operational examples are
        # normally darker through age, soot and an iron/mica undercoat.  The
        # factor is therefore the measured-looking in-service value, while the
        # material texture retains small fresher chips/highlights.
        "structure": (0.177780, 0.223228, 0.126000),
        "roughness": 0.79,
    },
    "eisengrau": {
        "label": "Eisengrau RAL 7011",
        "structure": (0.075739, 0.090655, 0.097530),
        "roughness": 0.78,
    },
    # The load-bearing metal stays iron-grey; the old mast itself receives the
    # red/white/black zone paint geometrically in lattice_mast().
    "altanstrich": {
        "label": "historischer Rot-Weiss-Schwarz-Anstrich",
        "structure": (0.075739, 0.090655, 0.097530),
        "roughness": 0.82,
    },
}


def materials_for(scheme):
    paint = PAINT_SCHEMES[scheme]
    return [
        material(MAT_STRUCTURE, paint["structure"], 0.02,
                 paint["roughness"], "painted"),
        # Siemens semaphore lamp backs are folded zinc-coated sheet, not a
        # continuation of the black optical barrel.  A deliberately dull
        # metallic response matches the weathered service covers in the
        # orthogonal Hertabruecke rear photographs without reading as chrome.
        # The weathered zinc skin has a substantial oxidised/dusty dielectric
        # fraction.  Keeping metalness below one is essential here: without a
        # reflection probe a pure-metal cover turns implausibly charcoal even
        # under the rear studio light, while the photographed housing remains
        # a diffuse pale grey.
        material(MAT_GALVANISED, (0.50, 0.52, 0.55), 0.30, 0.68,
                 "galvanised"),
        # Fluegelrueckseiten and blackened fittings are painted, not exposed
        # polished metal: high roughness and almost no metallic response.
        material(MAT_DARK, (0.0020, 0.0023, 0.0022), 0.02, 0.86,
                 "black-paint"),
        material(MAT_RED, (0.402937, 0.016754, 0.012335), 0.04, 0.48,
                 "enamel", clearcoat={
                     "factor": 0.32, "roughness": 0.34,
                     "normal_scale": 0.025,
                 }),
        material(MAT_WHITE, (0.680000, 0.680000, 0.620000), 0.02, 0.53,
                 "enamel", clearcoat={
                     "factor": 0.25, "roughness": 0.40,
                     "normal_scale": 0.020,
                 }),
        material(MAT_BLACK, (0.0019, 0.0019, 0.0023), 0.01, 0.84,
                 "black-paint"),
        # RAL 2000 display approximation converted from sRGB to linear.  The
        # former values were too red because the green channel had been
        # under-linearised; disc and matching Vr wing now retain one hue.
        material(MAT_YELLOW, (0.723055, 0.191202, 0.002125), 0.03, 0.50,
                 "enamel", clearcoat={
                     "factor": 0.30, "roughness": 0.36,
                     "normal_scale": 0.025,
                 }),
        material(MAT_VR_ORANGE, (0.723055, 0.191202, 0.002125), 0.02, 0.48,
                 "enamel", clearcoat={
                     "factor": 0.30, "roughness": 0.36,
                     "normal_scale": 0.025,
                 }),
        # Preserved DB 3.2-kg bottles are a noticeably pale sage green even
        # after service wear; this dedicated material must not inherit the
        # deliberately darker in-service mast colour.
        material(MAT_GAS_BOTTLE, (0.238398, 0.318547, 0.168269), 0.04, 0.69,
                 "painted"),
        material(MAT_RUST, (0.31, 0.075, 0.021), 0.05, 0.96, "rust"),
        material(MAT_GRIME, (0.075, 0.061, 0.044), 0.0, 0.98, "rubber"),
        material(MAT_CONCRETE, (0.43, 0.43, 0.39), 0.0, 0.92, "concrete"),
        material(MAT_NE2, (1.0, 1.0, 1.0), 0.0, 0.84,
                 "ne2-face", texture_size=1024),
        material("lit ruby glass", (0.62, 0.010, 0.008), 0.0, 0.09,
                 "optical-glass", (1.0, 0.025, 0.012)),
        material("lit green glass", (0.012, 0.55, 0.060), 0.0, 0.08,
                 "optical-glass", (0.025, 1.0, 0.12)),
        material("lit amber glass", (0.78, 0.38, 0.009), 0.0, 0.09,
                 "optical-glass", (1.0, 0.72, 0.025)),
        material("lit warm glass", (0.88, 0.81, 0.62), 0.0, 0.08,
                 "optical-glass", (1.0, 0.88, 0.60)),
        material("ruby filter glass", (0.22, 0.006, 0.005), 0.0, 0.075,
                 "optical-glass", glass={
                     "attenuation_colour": [0.62, 0.015, 0.010]}),
        # Unlit filters remain physically present but read as dark coloured
        # glass.  The separately switched source behind them supplies the night
        # aspect; otherwise all four apertures looked illuminated in daylight.
        material("green filter glass", (0.001, 0.055, 0.006), 0.0, 0.070,
                 "optical-glass", glass={
                     "attenuation_colour": [0.015, 0.48, 0.055]}),
        material("amber filter glass", (0.105, 0.030, 0.001), 0.0, 0.075,
                 "optical-glass", glass={
                     "attenuation_colour": [0.82, 0.37, 0.015]}),
        # The modern electric inserts use a thin, comparatively clear cover
        # pane.  Keep them separate from the deep moulded gas colour glasses:
        # less volume absorption and lower specular/Fresnel weight let the
        # dark reflector and LED source remain visible through the glass.
        # The green night pane reads blue-green in daylight (the warm source
        # behind it yields the prescribed green signal). Use the measured
        # appearance of the supplied front photographs rather than a dark
        # bottle-green paint colour; this is still a highly transmissive,
        # 1.5-mm optical pane, not an opaque cyan disc.
        material(MAT_LED_GREEN_FILTER, (0.006, 0.260, 0.520), 0.0, 0.022,
                 "led-optical-glass", glass={
                     "transmission": 0.94,
                     "ior": 1.48,
                     "thickness": 0.0015,
                     "attenuation_distance": 1.20,
                     "attenuation_colour": [0.07, 0.82, 0.96],
                     "specular": 0.82,
                 }),
        material(MAT_LED_AMBER_FILTER, (0.052, 0.014, 0.0005), 0.0, 0.022,
                 "led-optical-glass", glass={
                     "transmission": 0.91,
                     "ior": 1.48,
                     "thickness": 0.0015,
                     "attenuation_distance": 0.38,
                     "attenuation_colour": [0.88, 0.38, 0.018],
                     "specular": 0.68,
                 }),
        material(MAT_LED_LIT_GREEN, (0.006, 0.30, 0.28), 0.0, 0.018,
                 "led-optical-glass", (0.025, 1.0, 0.42),
                 glass={
                     "transmission": 0.76, "ior": 1.48,
                     "thickness": 0.0015, "attenuation_distance": 0.55,
                     "attenuation_colour": [0.04, 0.74, 0.72],
                     "specular": 0.68,
                 }),
        material(MAT_LED_LIT_AMBER, (0.42, 0.13, 0.002), 0.0, 0.020,
                 "led-optical-glass", (1.0, 0.60, 0.018),
                 glass={
                     "transmission": 0.74, "ior": 1.48,
                     "thickness": 0.0015, "attenuation_distance": 0.38,
                     "attenuation_colour": [0.88, 0.42, 0.018],
                     "specular": 0.68,
                 }),
    ]


def add(a, b):
    return tuple(a[i] + b[i] for i in range(3))


def mul(a, value):
    return tuple(a[i] * value for i in range(3))


def vector(a, b):
    return tuple(b[i] - a[i] for i in range(3))


def length(v):
    return math.sqrt(sum(c * c for c in v))


def unit(v):
    n = length(v)
    return tuple(c / n for c in v)


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0])


def beam(prim, start, end, radius, sides=8):
    """A capped round steel member between arbitrary points."""
    axis = unit(vector(start, end))
    reference = (0.0, 1.0, 0.0) if abs(axis[1]) < 0.88 else (1.0, 0.0, 0.0)
    u = unit(cross(axis, reference))
    v = cross(axis, u)
    ring0, ring1 = [], []
    for i in range(sides):
        angle = 2.0 * math.pi * i / sides
        offset = add(mul(u, radius * math.cos(angle)), mul(v, radius * math.sin(angle)))
        ring0.append(add(start, offset))
        ring1.append(add(end, offset))
    for i in range(sides):
        j = (i + 1) % sides
        prim.quad(ring0[i], ring1[i], ring1[j], ring0[j])
        prim.tri(start, ring0[j], ring0[i])
        prim.tri(end, ring1[i], ring1[j])


def rect_beam(prim, start, end, width, depth=None):
    """A capped rectangular flat/angle-steel member between two points.

    Signal masts are fabricated from rolled angles and flat bar.  Rendering
    every brace as a round rod was one of the main reasons the former lattice
    mast looked like a model-toy tower.  ``width`` is the visible face and
    ``depth`` its sheet/profile thickness.
    """
    depth = width if depth is None else depth
    axis = unit(vector(start, end))
    reference = (0.0, 0.0, 1.0) if abs(axis[2]) < 0.88 else (1.0, 0.0, 0.0)
    u = unit(cross(axis, reference))
    v = unit(cross(axis, u))
    u = mul(u, width * 0.5)
    v = mul(v, depth * 0.5)
    ring0 = [add(start, add(mul(u, sx), mul(v, sz)))
             for sx, sz in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
    ring1 = [add(end, add(mul(u, sx), mul(v, sz)))
             for sx, sz in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
    for i in range(4):
        j = (i + 1) % 4
        prim.quad(ring0[i], ring1[i], ring1[j], ring0[j])
    prim.quad(ring0[3], ring0[2], ring0[1], ring0[0])
    prim.quad(ring1[0], ring1[1], ring1[2], ring1[3])


def cylinder(prim, centre, radius, depth, axis=(0.0, 0.0, 1.0), sides=24):
    half = mul(unit(axis), depth * 0.5)
    beam(prim, add(centre, mul(half, -1.0)), add(centre, half), radius, sides)


def frustum_y(prim, centre_xz, y0, y1, radius0, radius1, sides=24):
    """A capped circular frustum whose axis follows the model's Y axis."""
    cx, cz = centre_xz
    lower = [(cx + radius0 * math.cos(2.0 * math.pi * i / sides),
              y0,
              cz + radius0 * math.sin(2.0 * math.pi * i / sides))
             for i in range(sides)]
    upper = [(cx + radius1 * math.cos(2.0 * math.pi * i / sides),
              y1,
              cz + radius1 * math.sin(2.0 * math.pi * i / sides))
             for i in range(sides)]
    lower_centre, upper_centre = (cx, y0, cz), (cx, y1, cz)
    for i in range(sides):
        nxt = (i + 1) % sides
        prim.quad(lower[i], upper[i], upper[nxt], lower[nxt])
        prim.tri(lower_centre, lower[nxt], lower[i])
        prim.tri(upper_centre, upper[i], upper[nxt])


def hoop_xz(prim, centre, radius, rod_radius, segments=24):
    """Open round retaining hoop in an XZ plane, assembled from round rod."""
    cx, cy, cz = centre
    points = [(cx + radius * math.cos(2.0 * math.pi * i / segments),
               cy,
               cz + radius * math.sin(2.0 * math.pi * i / segments))
              for i in range(segments)]
    for i, point in enumerate(points):
        beam(prim, point, points[(i + 1) % segments], rod_radius,
             max(5, min(8, segments // 3)))


def coil_spring_y(prim, centre_xz, y0, y1, radius, turns,
                  wire_radius, segments_per_turn=10):
    """A continuous helical spring whose axis follows model Y.

    The returned endpoints are useful for adding real attachment hooks.  A
    helix is intentionally built from connected beams instead of stacked
    rings: gaps between the old rings were the visible "cut cable" defect in
    close front and rear views.
    """
    cx, cz = centre_xz
    segments = max(4, round(turns * segments_per_turn))
    points = []
    for index in range(segments + 1):
        t = index / segments
        angle = 2.0 * math.pi * turns * t
        points.append((cx + radius * math.cos(angle),
                       y0 + (y1 - y0) * t,
                       cz + radius * math.sin(angle)))
    sides = max(5, min(8, segments_per_turn))
    for start, end in zip(points, points[1:]):
        beam(prim, start, end, wire_radius, sides)
    return points[0], points[-1]


def chamfered_box_xy(prim, centre, width, height, z0, z1, chamfer=0.025):
    """Extrude a rectangular cast cover with clipped corners."""
    cx, cy = centre
    hx, hy = width * 0.5, height * 0.5
    c = min(chamfer, hx * 0.45, hy * 0.45)
    extrude_xy(prim, [
        (cx - hx + c, cy - hy), (cx + hx - c, cy - hy),
        (cx + hx, cy - hy + c), (cx + hx, cy + hy - c),
        (cx + hx - c, cy + hy), (cx - hx + c, cy + hy),
        (cx - hx, cy + hy - c), (cx - hx, cy - hy + c),
    ], z0, z1)


def diamond_plate(prim, centre, half, z0, z1):
    """Small square counterweight rotated 45 degrees in the signal plane."""
    cx, cy = centre
    extrude_xy(prim, [(cx, cy + half), (cx - half, cy),
                      (cx, cy - half), (cx + half, cy)], z0, z1)


def optical_lens(prim, centre, radius, z_back, sides=64, rings=7,
                 edge_depth=0.003, crown=0.019, fresnel_step=0.0007,
                 centre_depth=0.023):
    """Closed, shallow-convex signal lens with restrained Fresnel stepping."""
    cx, cy = centre
    front_rings = []
    for ring in range(1, rings + 1):
        t = ring / rings
        ring_radius = radius * t
        # A very shallow alternating step catches highlights like moulded signal
        # glass without turning the lens into coarse concentric geometry.
        z = z_back + edge_depth + crown * (1.0 - t * t)
        z += fresnel_step if ring % 2 == 0 else 0.0
        front_rings.append([
            (cx + ring_radius * math.cos(2.0 * math.pi * i / sides),
             cy + ring_radius * math.sin(2.0 * math.pi * i / sides), z)
            for i in range(sides)
        ])
    front_centre = (cx, cy, z_back + centre_depth)
    first = front_rings[0]
    for i in range(sides):
        j = (i + 1) % sides
        prim.tri(front_centre, first[i], first[j])
    for inner, outer in zip(front_rings, front_rings[1:]):
        for i in range(sides):
            j = (i + 1) % sides
            prim.quad(inner[i], outer[i], outer[j], inner[j])
    back_centre = (cx, cy, z_back)
    back_ring = [(cx + radius * math.cos(2.0 * math.pi * i / sides),
                  cy + radius * math.sin(2.0 * math.pi * i / sides), z_back)
                 for i in range(sides)]
    outer = front_rings[-1]
    for i in range(sides):
        j = (i + 1) % sides
        prim.tri(back_centre, back_ring[j], back_ring[i])
        prim.quad(back_ring[i], back_ring[j], outer[j], outer[i])


def extrude_xy(prim, points, z0, z1):
    """Extrude a convex counter-clockwise XY polygon."""
    centre = (sum(p[0] for p in points) / len(points),
              sum(p[1] for p in points) / len(points))
    for i, point in enumerate(points):
        nxt = points[(i + 1) % len(points)]
        prim.tri((centre[0], centre[1], z1), (point[0], point[1], z1),
                 (nxt[0], nxt[1], z1))
        prim.tri((centre[0], centre[1], z0), (nxt[0], nxt[1], z0),
                 (point[0], point[1], z0))
        prim.quad((point[0], point[1], z0), (nxt[0], nxt[1], z0),
                  (nxt[0], nxt[1], z1), (point[0], point[1], z1))


def extrude_yz(prim, points, x0, x1):
    """Extrude a convex YZ polygon along X.

    ``extrude_xy`` remains the single triangulation implementation.  A cyclic
    axis permutation preserves its winding and lets side-facing stamped plates
    use broad flat geometry instead of round beams.
    """
    temporary = Prim()
    extrude_xy(temporary, points, x0, x1)
    base = len(prim.pos)
    prim.pos.extend((z, x, y) for x, y, z in temporary.pos)
    prim.nrm.extend((z, x, y) for x, y, z in temporary.nrm)
    prim.uv.extend(temporary.uv)
    prim.idx.extend(base + index for index in temporary.idx)


def convex_hull_xy(points):
    """Return a counter-clockwise convex hull for a set of XY points."""
    ordered = sorted(set(points))
    if len(ordered) <= 1:
        return ordered

    def turn(a, b, c):
        return ((b[0] - a[0]) * (c[1] - a[1])
                - (b[1] - a[1]) * (c[0] - a[0]))

    lower = []
    for point in ordered:
        while len(lower) >= 2 and turn(lower[-2], lower[-1], point) <= 0.0:
            lower.pop()
        lower.append(point)
    upper = []
    for point in reversed(ordered):
        while len(upper) >= 2 and turn(upper[-2], upper[-1], point) <= 0.0:
            upper.pop()
        upper.append(point)
    return lower[:-1] + upper[:-1]


def capsule_y(prim, centre, width, height, z0, z1, sides=16):
    """Extrude a vertically oriented rounded-rectangle/capsule housing."""
    cx, cy = centre
    radius = width * 0.5
    straight = max(0.0, height * 0.5 - radius)
    half = max(4, sides // 2)
    points = []
    for i in range(half + 1):
        a = math.pi * i / half
        points.append((cx + radius * math.cos(a),
                       cy + straight + radius * math.sin(a)))
    for i in range(half + 1):
        a = math.pi + math.pi * i / half
        points.append((cx + radius * math.cos(a),
                       cy - straight + radius * math.sin(a)))
    extrude_xy(prim, points, z0, z1)


def annulus(prim, centre, outer, inner, z0, z1, sides=32):
    cx, cy = centre
    for i in range(sides):
        a0, a1 = 2 * math.pi * i / sides, 2 * math.pi * (i + 1) / sides
        o0, o1 = (cx + outer * math.cos(a0), cy + outer * math.sin(a0)), (cx + outer * math.cos(a1), cy + outer * math.sin(a1))
        i0, i1 = (cx + inner * math.cos(a0), cy + inner * math.sin(a0)), (cx + inner * math.cos(a1), cy + inner * math.sin(a1))
        prim.quad((i0[0], i0[1], z1), (o0[0], o0[1], z1), (o1[0], o1[1], z1), (i1[0], i1[1], z1))
        prim.quad((o0[0], o0[1], z0), (i0[0], i0[1], z0), (i1[0], i1[1], z0), (o1[0], o1[1], z0))
        prim.quad((o0[0], o0[1], z0), (o1[0], o1[1], z0), (o1[0], o1[1], z1), (o0[0], o0[1], z1))
        prim.quad((i1[0], i1[1], z0), (i0[0], i0[1], z0), (i0[0], i0[1], z1), (i1[0], i1[1], z1))


def annulus_arc(prim, centre, outer, inner, z0, z1,
                start_radians, end_radians, segments=16):
    """Extrude an open annular arc in the XY plane, including end caps."""
    cx, cy = centre
    rings = []
    for index in range(segments + 1):
        angle = start_radians + (end_radians - start_radians) * index / segments
        rings.append((
            (cx + inner * math.cos(angle), cy + inner * math.sin(angle)),
            (cx + outer * math.cos(angle), cy + outer * math.sin(angle)),
        ))
    for index in range(segments):
        (i0, o0), (i1, o1) = rings[index], rings[index + 1]
        prim.quad((i0[0], i0[1], z1), (o0[0], o0[1], z1),
                  (o1[0], o1[1], z1), (i1[0], i1[1], z1))
        prim.quad((o0[0], o0[1], z0), (i0[0], i0[1], z0),
                  (i1[0], i1[1], z0), (o1[0], o1[1], z0))
        prim.quad((o0[0], o0[1], z0), (o1[0], o1[1], z0),
                  (o1[0], o1[1], z1), (o0[0], o0[1], z1))
        prim.quad((i1[0], i1[1], z0), (i0[0], i0[1], z0),
                  (i0[0], i0[1], z1), (i1[0], i1[1], z1))
    for inner_point, outer_point in (rings[0], rings[-1]):
        prim.quad((inner_point[0], inner_point[1], z0),
                  (outer_point[0], outer_point[1], z0),
                  (outer_point[0], outer_point[1], z1),
                  (inner_point[0], inner_point[1], z1))


def spoked_wheel_yz(prim, centre, radius, lod=0):
    """Stamped three-spoke cable wheel in the signal's side plane.

    Historical Fig. 122 shows a wide flat rim and three broad sheet webs, not
    round rods.  Segment-wise YZ plates retain the three large openings while
    giving the wheel the heavy silhouette seen on surviving mechanisms.
    """
    cx, cy, cz = centre
    segments = (32, 18, 10)[lod]
    x0, x1 = cx - 0.031, cx + 0.031
    rim_inner = radius * 0.745
    for index in range(segments):
        a0 = 2.0 * math.pi * index / segments
        a1 = 2.0 * math.pi * (index + 1) / segments
        extrude_yz(prim, [
            (cy + rim_inner * math.cos(a0),
             cz + rim_inner * math.sin(a0)),
            (cy + radius * math.cos(a0),
             cz + radius * math.sin(a0)),
            (cy + radius * math.cos(a1),
             cz + radius * math.sin(a1)),
            (cy + rim_inner * math.cos(a1),
             cz + rim_inner * math.sin(a1)),
        ], x0, x1)
    if lod < 2:
        hub_radius = radius * 0.245
        for index in range(3):
            angle = math.radians(90.0 + index * 120.0)
            radial = (math.cos(angle), math.sin(angle))
            tangent = (-radial[1], radial[0])
            inner_r, outer_r = hub_radius * 0.70, rim_inner * 1.04
            inner_half, outer_half = radius * 0.105, radius * 0.155
            extrude_yz(prim, [
                (cy + radial[0] * inner_r - tangent[0] * inner_half,
                 cz + radial[1] * inner_r - tangent[1] * inner_half),
                (cy + radial[0] * outer_r - tangent[0] * outer_half,
                 cz + radial[1] * outer_r - tangent[1] * outer_half),
                (cy + radial[0] * outer_r + tangent[0] * outer_half,
                 cz + radial[1] * outer_r + tangent[1] * outer_half),
                (cy + radial[0] * inner_r + tangent[0] * inner_half,
                 cz + radial[1] * inner_r + tangent[1] * inner_half),
            ], x0 - 0.002, x1 + 0.002)
        cylinder(prim, centre, hub_radius, 0.070, axis=(1, 0, 0),
                 sides=(32, 18)[lod])
    else:
        cylinder(prim, centre, radius * 0.30, 0.070, axis=(1, 0, 0),
                 sides=10)
    cylinder(prim, centre, 0.028, 0.090, axis=(1, 0, 0),
             sides=(20, 12, 8)[lod])


def irregular_chip(prim, x, y, width, height, z):
    """A thin, non-rectangular paint chip on a train-facing XY surface."""
    points = [(x - width * 0.52, y - height * 0.18),
              (x - width * 0.23, y - height * 0.55),
              (x + width * 0.44, y - height * 0.34),
              (x + width * 0.53, y + height * 0.16),
              (x + width * 0.10, y + height * 0.52),
              (x - width * 0.46, y + height * 0.30)]
    extrude_xy(prim, points, z, z + 0.004)


def foundation(prims, wide=0.58):
    concrete = prims["concrete"]
    steel, dark = prims["steel"], prims["dark"]
    rust, grime = prims["rust"], prims["grime"]
    # Only the upper shoulder of the poured foundation is above rail level.
    # The former three-storey plinth looked like a model-railway mounting base.
    concrete.box((-wide / 2, -0.22, -wide / 2),
                 (wide / 2, 0.08, wide / 2))
    steel.box((-wide * 0.40, 0.08, -wide * 0.40),
              (wide * 0.40, 0.125, wide * 0.40))
    grime.box((-wide / 2 - 0.006, -0.025, -wide / 2 - 0.006),
              (wide / 2 + 0.006, 0.020, wide / 2 + 0.006))
    for x in (-wide * 0.27, wide * 0.27):
        for z in (-wide * 0.27, wide * 0.27):
            cylinder(dark, (x, 0.155, z), 0.024, 0.060,
                     axis=(0, 1, 0), sides=12)
            cylinder(rust, (x, 0.128, z), 0.027, 0.006,
                     axis=(0, 1, 0), sides=12)


def mast_paint_key(y, scheme):
    """Material bucket for a load-bearing mast member at height *y*."""
    if scheme != "altanstrich":
        return "steel"
    if y < 3.0:
        return "black"
    # The historic paint scheme is not made from the later enamel mast plates.
    # Its broad red fields and shorter white separators follow the same visual
    # rhythm as the preserved signals used for this reconstruction.
    period = HP_RED_BOARD_LENGTH + HP_WHITE_BOARD_LENGTH
    return ("red" if (y - 3.0) % period < HP_RED_BOARD_LENGTH else "white")


def mast_paint_segments(start, end, scheme):
    if scheme != "altanstrich":
        return [(start, end, "steel")]
    cuts = {start, end}
    boundary = 3.0
    red_phase = True
    while boundary < end:
        if start < boundary:
            cuts.add(boundary)
        boundary += (HP_RED_BOARD_LENGTH if red_phase
                     else HP_WHITE_BOARD_LENGTH)
        red_phase = not red_phase
    ordered = sorted(cuts)
    return [(a, b, mast_paint_key((a + b) * 0.5, scheme))
            for a, b in zip(ordered, ordered[1:])]


def lattice_mast(prims, height, narrow=False, scheme="db_gruen", lod=0):
    steel, rust, grime = prims["steel"], prims["rust"], prims["grime"]
    if narrow:
        # Welded Schmalmast: 100 mm in the train-facing elevation and about
        # 250 mm deep.  Preserved rear elevations and the folded mast assembly
        # show two longitudinal rails with rectangular openings; the former
        # continuous front/rear sheets incorrectly turned it into a solid post.
        half_width = HP_SCHMAL_MAST_WIDTH * 0.5
        half_depth = HP_SCHMAL_MAST_DEPTH * 0.5
        plate = HP_SCHMAL_MAST_PLATE
        for y0, y1, key in mast_paint_segments(0.17, height, scheme):
            member = prims[key]
            for z in (-half_depth + plate * 0.5,
                      half_depth - plate * 0.5):
                for x in (-half_width + plate * 0.5,
                          half_width - plate * 0.5):
                    member.box((x - plate * 0.5, y0, z - plate * 0.5),
                               (x + plate * 0.5, y1, z + plate * 0.5))
        cross_step = 0.38 * (1, 2, 4)[lod]
        for y in [0.27 + i * cross_step
                  for i in range(int((height - 0.25) / cross_step) + 1)]:
            member = prims[mast_paint_key(y, scheme)]
            # One shallow tie on each train-facing frame, plus the matching
            # side diaphragms that weld both frames into the narrow box mast.
            for z in (-half_depth + plate * 0.5,
                      half_depth - plate * 0.5):
                member.box((-half_width, y - 0.012, z - plate * 0.5),
                           (half_width, y + 0.012, z + plate * 0.5))
            for x in (-half_width + plate * 0.5,
                      half_width - plate * 0.5):
                member.box((x - plate * 0.5, y - 0.012, -half_depth),
                           (x + plate * 0.5, y + 0.012, half_depth))
        if lod == 0:
            for y in (0.28, min(2.36, height - 0.2),
                      min(4.46, height - 0.2)):
                rust.box((-half_width - 0.0005, y, -half_depth - 0.001),
                         (half_width + 0.0005, y + 0.006,
                          half_depth + 0.001))

        def mast_half_at(_y):
            return half_width

        def mast_depth_at(_y):
            return half_depth
    else:
        # Four-angle Einheits-Gittermast.  The longitudinal chords and braces
        # are flat/angle steel, not round tubing.  Each physical face has one
        # alternating diagonal per bay.  Opposing faces are mirrored: viewed
        # through the open mast they therefore form the crossed projection in
        # Abb. 107, while an oblique view still reads as four separate zig-zags.
        bottom_half, top_half = 0.205, 0.145
        bottom_depth, top_depth = 0.170, 0.115

        def mast_half_at(y):
            t = max(0.0, min(1.0, (y - 0.17) / max(0.01, height - 0.17)))
            return bottom_half + (top_half - bottom_half) * t

        def mast_depth_at(y):
            t = max(0.0, min(1.0, (y - 0.17) / max(0.01, height - 0.17)))
            return bottom_depth + (top_depth - bottom_depth) * t

        for y0, y1, key in mast_paint_segments(0.17, height, scheme):
            member = prims[key]
            for sx in (-1, 1):
                for sz in (-1, 1):
                    x0, x1 = mast_half_at(y0), mast_half_at(y1)
                    z0, z1 = mast_depth_at(y0), mast_depth_at(y1)
                    rect_beam(member, (sx * x0, y0, sz * z0),
                              (sx * x1, y1, sz * z1),
                              0.030 if lod == 0 else 0.034, 0.024)
        bays = max(4, round((height - 0.2) /
                            (HP_GITTER_BAY_PITCH * (1, 2, 4)[lod])))
        for bay in range(bays):
            y0 = 0.18 + (height - 0.18) * bay / bays
            y1 = 0.18 + (height - 0.18) * (bay + 1) / bays
            half0, half1 = mast_half_at(y0), mast_half_at(y1)
            depth0, depth1 = mast_depth_at(y0), mast_depth_at(y1)
            member = prims[mast_paint_key((y0 + y1) * 0.5, scheme)]
            for zsign in (-1, 1):
                direction = hp_gitter_brace_direction(bay, zsign)
                rect_beam(member,
                          (-direction * half0, y0, zsign * depth0),
                          (direction * half1, y1, zsign * depth1),
                          0.027, 0.010)
                rect_beam(member, (-half0, y0, zsign * depth0),
                          (half0, y0, zsign * depth0), 0.025, 0.010)
            for xsign in (-1, 1):
                direction = hp_gitter_brace_direction(bay, xsign)
                rect_beam(member,
                          (xsign * half0, y0, -direction * depth0),
                          (xsign * half1, y1, direction * depth1),
                          0.027, 0.010)
                rect_beam(member, (xsign * half0, y0, -depth0),
                          (xsign * half0, y0, depth0), 0.025, 0.010)
        if lod == 0:
            for y in (0.20, min(2.35, height - 0.1),
                      min(5.10, height - 0.1)):
                half_y, depth_y = mast_half_at(y), mast_depth_at(y)
                for x in (-half_y, half_y):
                    rust.box((x - 0.015, y, depth_y - 0.018),
                             (x + 0.015, y + 0.009, depth_y + 0.004))

    # Small alternating rear-quarter climbing irons.  Their former 245-mm
    # projection dominated the elevation; the reference reads closer to 130 mm.
    rung_step = 0.36 * (1, 2, 4)[lod]
    for index, y in enumerate(
            0.44 + i * rung_step
            for i in range(int((height - 0.70) / rung_step) + 1)):
        half_y = mast_half_at(y)
        depth_y = mast_depth_at(y)
        rung_root = half_y + 0.004
        rung_tip = half_y + (0.135 if not narrow else 0.125)
        side = 1.0 if index % 2 == 0 else -1.0
        member = prims[mast_paint_key(y, scheme)]
        rung_z = -depth_y - 0.008
        rect_beam(member, (side * rung_root, y, rung_z),
                  (side * rung_tip, y, rung_z), 0.014, 0.008)
        rect_beam(member, (side * rung_tip, y, rung_z),
                  (side * rung_tip, y + 0.040, rung_z), 0.014, 0.008)
    # Lantern-hoist cables are emitted with the rear mechanism, not baked into
    # the load-bearing mast.  This keeps their two ends tied to the mast-head
    # wheel and mast foot and prevents a mast-height edit from leaving them
    # suspended several decimetres below the pulley.
    grime.box((-0.23, 0.125, -0.21), (0.23, 0.185, 0.21))


def vr_mast_1944(prims, height, scheme="db_gruen", lod=0):
    """Dedicated Einheit-Vorsignalmast Bauart 1944.

    The preserved J35-326 description explicitly distinguishes this mast from
    the older two-channel construction: it is one double-T section whose web
    runs parallel to the track.  With Z as the track/front axis, the web is a
    continuous YZ plate.  The I-profile recesses are consequently visible only
    from the left and right, never as ladder openings in the train-facing view.

    Overall 100 x 250 mm and the 12-mm plate thickness are class-C photo/profile
    reconstructions; the orientation and continuous topology are source-backed.
    This function is deliberately separate from ``lattice_mast(narrow=True)``
    so a Vorsignal correction cannot modify a Hauptsignal Schmalmast again.
    """
    member = prims[mast_paint_key((0.17 + height) * 0.5, scheme)]
    half_width = VR_1944_MAST_WIDTH * 0.5
    half_depth = VR_1944_MAST_DEPTH * 0.5
    flange_thickness = VR_1944_MAST_PLATE
    web_half = VR_1944_MAST_PLATE * 0.5
    y0 = 0.17

    # Front and rear flanges plus one uninterrupted web parallel to the track.
    member.box((-half_width, y0, half_depth - flange_thickness),
               (half_width, height, half_depth))
    member.box((-half_width, y0, -half_depth),
               (half_width, height, -half_depth + flange_thickness))
    member.box((-web_half, y0, -half_depth + flange_thickness),
               (web_half, height, half_depth - flange_thickness))

    # Real mounting/splice plates are sparse and flush with the side recess;
    # they are not the repeating rungs of the former ladder-like substitute.
    if lod < 2:
        for y in (0.30, min(height - 0.28, 2.42)):
            if y0 + 0.08 < y < height - 0.08:
                member.box((-half_width - 0.006, y - 0.026,
                            -half_depth + 0.018),
                           (half_width + 0.006, y + 0.026,
                            half_depth - 0.018))
    if lod == 0:
        # Two restrained drain/rust lines and one low service step reproduce
        # plausible maintenance wear without inventing a full ladder.
        for z in (-half_depth, half_depth - 0.004):
            prims["rust"].box((-half_width, 0.292, z),
                              (half_width, 0.300, z + 0.004))
        rect_beam(member, (half_width, 0.62, -half_depth - 0.006),
                  (half_width + 0.115, 0.62, -half_depth - 0.006),
                  0.014, 0.008)
        rect_beam(member,
                  (half_width + 0.115, 0.62, -half_depth - 0.006),
                  (half_width + 0.115, 0.66, -half_depth - 0.006),
                  0.014, 0.008)
    prims["grime"].box((-0.12, 0.125, -0.17),
                       (0.12, 0.185, 0.17))


def vr_mast_old_u(prims, height, scheme="db_gruen", lod=0):
    """Older Einheit construction made from two separated U channels.

    J35-326 and the surviving Germersheim signal show two continuous uprights
    and sparse ties in the train-facing elevation.  This is the documented
    elaborate old construction, not a four-chord lattice mast.  Envelope and
    channel gauge remain class-C photo reconstructions.
    """
    member = prims[mast_paint_key((0.17 + height) * 0.5, scheme)]
    half_width = VR_OLD_U_MAST_WIDTH * 0.5
    half_depth = VR_OLD_U_MAST_DEPTH * 0.5
    plate = VR_OLD_U_MAST_PLATE
    gap_half = VR_OLD_U_MAST_GAP * 0.5
    y0 = 0.17

    # Two channels open towards the centre.  Their outer webs span the full
    # front/rear depth; paired flanges finish at the clear central slot.
    member.box((-half_width, y0, -half_depth),
               (-half_width + plate, height, half_depth))
    member.box((half_width - plate, y0, -half_depth),
               (half_width, height, half_depth))
    for z0, z1 in ((-half_depth, -half_depth + plate),
                   (half_depth - plate, half_depth)):
        member.box((-half_width + plate, y0, z0),
                   (-gap_half, height, z1))
        member.box((gap_half, y0, z0),
                   (half_width - plate, height, z1))

    tie_step = 0.46 * (1, 2, 4)[lod]
    tie_count = int((height - 0.30) / tie_step) + 1
    for index in range(tie_count):
        y = 0.30 + index * tie_step
        if y >= height - 0.08:
            continue
        # Narrow plates bridge the two channels behind the working slot; the
        # openings between them remain unmistakable from the signal front.
        member.box((-half_width, y - 0.018, -0.018),
                   (half_width, y + 0.018, 0.018))
    if lod == 0:
        # A single low stirrup and restrained corrosion at the tie edges.
        rect_beam(member, (half_width, 0.60, -half_depth - 0.006),
                  (half_width + 0.115, 0.60, -half_depth - 0.006),
                  0.014, 0.008)
        rect_beam(member,
                  (half_width + 0.115, 0.60, -half_depth - 0.006),
                  (half_width + 0.115, 0.64, -half_depth - 0.006),
                  0.014, 0.008)
        for y in (0.30, min(2.14, height - 0.12)):
            prims["rust"].box((-half_width, y - 0.004, -0.020),
                              (half_width, y + 0.004, 0.020))
    prims["grime"].box((-0.14, 0.125, -0.15),
                       (0.14, 0.185, 0.15))


def mast_board(prims, height, mast, lod=0):
    # Full-size form-signal recognition plates are separate enamel sheets, not
    # a stack of 800-mm white-red-white light-signal mast signs.  The current
    # supplier catalogue retains 998-mm red *and* white plates in 100/200-mm
    # widths.  Orthogonal 8-m elevations confirm five equal alternating fields;
    # the family consequently uses 3/5/7/9/11 fields for the nominal
    # 6/8/10/12/14-m constructions, always beginning and ending red.  ``height``
    # is already expressed above SO here; rounding
    # still recovers the nominal even metre after the 172-mm datum conversion.
    top = height - 0.46
    # The enamelled sequence does not run to the foundation.  Orthogonal 8-m
    # prototype views show five 998-mm fields and about 2.38 m of bare lattice
    # below them (including the designation plate).  Adding one red/white pair
    # for every further two metres preserves that common lower termination on
    # the documented 6/8/10/12/14-m family.
    field_count = max(3, int(round(height)) - 3)
    if field_count % 2 == 0:
        field_count -= 1
    half_width = MAST_BOARD_WIDTH[mast] * 0.5
    z0 = 0.178 if mast == "gitter" else 0.146
    y1 = top
    plates = []
    for index in range(field_count):
        colour = "red" if index % 2 == 0 else "white"
        length = (HP_RED_BOARD_LENGTH if colour == "red"
                  else HP_WHITE_BOARD_LENGTH)
        y0 = y1 - length
        # The colour is enamel on the train-facing skin, not paint through the
        # full sheet.  Keep a separate dark folded backing so a rear view does
        # not incorrectly reproduce the red/white front pattern.
        prims["dark"].box((-half_width, y0, z0),
                          (half_width, y1, z0 + 0.012))
        prims[colour].box((-half_width, y0, z0 + 0.012),
                          (half_width, y1, z0 + 0.016))
        plates.append((colour, y0, y1))
        y1 = y0
    if lod < 2:
        # Separate rear straps and folded lips; there is no continuous thick
        # backing board and no row of invented orange fasteners.
        for _colour, y0, _y1 in plates:
            prims["dark"].box((-half_width - 0.010, y0 - 0.009,
                               z0 - 0.012),
                              (half_width + 0.010, y0 + 0.009,
                               z0 + 0.003))
        prims["dark"].box((-half_width - 0.007, y1, z0 - 0.005),
                          (-half_width + 0.003, top, z0 + 0.010))
        prims["dark"].box((half_width - 0.003, y1, z0 - 0.005),
                          (half_width + 0.007, top, z0 + 0.010))


def rotate_prim_z(prim, angle):
    c, s = math.cos(angle), math.sin(angle)
    prim.pos = [(c * x - s * y, s * x + c * y, z) for x, y, z in prim.pos]
    prim.nrm = [(c * x - s * y, s * x + c * y, z) for x, y, z in prim.nrm]


def main_arm_mesh(lower=False, shortened=False, negative=False, lod=0):
    """DB S 050.07.2 blade; catalogue length includes the compact root."""
    red, white, dark, steel, rust = Prim(), Prim(), Prim(), Prim(), Prim()
    blade_len, blade_width, disc_diameter = HP_ARM[(lower, shortened)]
    root = HP_ARM_ROOT[lower]
    disc_radius = disc_diameter * 0.5
    blade_tip = blade_len - root
    disc_x = blade_tip - disc_radius
    half_h = blade_width * 0.5
    sides = (72, 36, 18)[lod]
    # The SWB S 050.07.2 product silhouette and orthogonal prototype views put
    # the white enamel field at roughly half the rectangular blade depth.  The
    # previous 56--62 percent field made the red frame implausibly
    # thin and was immediately visible in the running demo.
    white_half = 0.060 if not lower else 0.055
    white_disc = HP_ARM_WHITE_DISC_RADIUS[lower]
    stripe_end = hp_arm_stripe_end(lower, shortened)

    # Folded black rear channel is a few millimetres larger than the enamelled
    # face, hence the fine dark outline seen from front and side.
    dark.box((-root, -half_h, -0.024), (disc_x, half_h, 0.002))
    cylinder(dark, (disc_x, 0.0, -0.011), disc_radius, 0.026, sides=sides)
    red.box((-root + 0.008, -half_h + 0.006, 0.003),
            (disc_x, half_h - 0.006, 0.038))
    cylinder(red, (disc_x, 0.0, 0.020), disc_radius - 0.004,
             0.035, sides=sides)

    # The straight field ends at the left tangent of the outer disc.  Prototype
    # fronts and the S 050.07.2 supplier silhouette consistently retain a
    # closed red ring between this field and the separate white circular
    # insert; continuing the rectangle under the insert visibly breaks it.
    white.box((-root + 0.060, -white_half, 0.039),
              (stripe_end, white_half, 0.046))
    cylinder(white, (disc_x, 0.0, 0.043), white_disc, 0.008,
             sides=max(16, sides - 8))

    # Rear: cream enamel field in a black U-channel with raised upper/lower
    # rails.  It is not a mirrored red front face.  Prototype rear elevations
    # show an uninterrupted enamel field; the structural fasteners belong to
    # the bearing and linkage at the mast, not as decorative dots on the arm.
    rear_white = red if negative else white
    rear_white.box((-root + 0.060, -white_half, -0.032),
                   (stripe_end, white_half, -0.025))
    cylinder(rear_white, (disc_x, 0.0, -0.029), white_disc, 0.007,
             sides=max(16, sides - 8))
    if lod < 2:
        rail_depth = 0.012 if lod == 0 else 0.016
        dark.box((-root + 0.040, -half_h - 0.010, -0.038),
                 (disc_x - 0.155, -half_h + 0.030, -0.024))
        dark.box((-root + 0.040, half_h - 0.030, -0.038),
                 (disc_x - 0.155, half_h + 0.010, -0.024))

    # Actual bearing at the centre line plus inner tail/stop iron.  The old
    # 164-mm black sphere hid the mast and made the blade appear toy-like.
    cylinder(dark, (0.0, 0.0, -0.050), 0.057, 0.070,
             sides=(32, 20, 12)[lod])
    cylinder(steel, (0.0, 0.0, -0.090), 0.024, 0.018,
             sides=(20, 14, 8)[lod])
    cylinder(dark, (0.0, 0.0, 0.052), 0.034, 0.014,
             sides=(24, 16, 10)[lod])
    if lod < 2:
        # Slotted coupling link hangs from the inner end behind the blade.
        beam(dark, (-root + 0.075, -half_h - 0.015, -0.060),
             (-root + 0.145, -half_h - 0.155, -0.105),
             0.013, max(6, 9 - lod * 2))
        cylinder(steel, (-root + 0.145, -half_h - 0.155, -0.105),
                 0.023, 0.015, sides=(16, 10)[lod])
    if lod == 0:
        # Wear is confined to the folded lower edge/root water trap; no orange
        # dots are placed on the enamel field.
        rust.box((-root + 0.030, -half_h - 0.002, -0.004),
                 (-root + 0.205, -half_h + 0.006, 0.007))
    if lower:
        # Hp 0/Hp 1: the second blade stands vertically upward.  For Hp 2 it
        # lowers by 45° to the same right-rising position as the upper blade.
        # The round blade head is the free end, not the bearing at the mast.
        for prim in (red, white, dark, steel, rust):
            rotate_prim_z(prim, math.pi / 2.0)
    face_red, face_white = ((white, red) if negative else (red, white))
    return {MAT_RED: face_red, MAT_WHITE: face_white, MAT_DARK: dark,
            MAT_STRUCTURE: steel, MAT_RUST: rust}


def hp_balance_mesh(lower=False, lod=0):
    """Rear blade-holder weight, rotating with its Hp blade.

    The enamel blade is mounted in front of the mast, whereas this forged
    assembly is carried by the same through-shaft on the rear.  Keeping it in
    a separate node preserves both real depth planes while allowing the exact
    same motion sample (including a halt-fall rebound) to drive both.
    """
    dark, steel = Prim(), Prim()
    rear_z = -0.252

    if not lower:
        # Flat, irregular mounting cheek behind the blade root.  Its short
        # Z-offset lever ends in one transverse hammer weight made from three
        # laminations; the real fitting is compact and has no decorative
        # orange fasteners or dangling pair of diamond plates.
        extrude_xy(dark, [(-0.080, 0.052), (0.065, 0.040),
                          (0.092, -0.038), (-0.020, -0.108)],
                   rear_z - 0.034, rear_z + 0.018)
        cylinder(dark, (0.0, 0.0, rear_z - 0.010), 0.044, 0.066,
                 sides=(24, 16, 10)[lod])
        # LOD0 already owns the neutral-metal detail slot.  The coarser LODs
        # keep their original one-material layout so a silhouette correction
        # cannot quietly change batching/PBR bindings.
        axle = steel if lod == 0 else dark
        cylinder(axle, (0.0, 0.0, rear_z - 0.048), 0.017, 0.010,
                 sides=(18, 12, 8)[lod])
        bend_x, bend_y = HP_UPPER_HOLDER_BEND
        weight_x, weight_y = HP_UPPER_HOLDER_WEIGHT_CENTRE
        rect_beam(dark, (-0.018, -0.032, rear_z - 0.040),
                  (bend_x, bend_y, rear_z - 0.054), 0.030, 0.018)
        rect_beam(dark, (bend_x, bend_y, rear_z - 0.054),
                  (weight_x, weight_y, rear_z - 0.064), 0.026, 0.016)

        dx, dy = weight_x - bend_x, weight_y - bend_y
        length = math.hypot(dx, dy)
        # The hammer lies transverse to the last lever segment.  Three shallow
        # layers reproduce the visibly built-up weight without turning it into
        # three separate blocks in silhouette.
        px = -dy / length * HP_UPPER_HOLDER_WEIGHT_SPAN * 0.5
        py = dx / length * HP_UPPER_HOLDER_WEIGHT_SPAN * 0.5
        for layer in range(3):
            z = rear_z - 0.060 - layer * 0.012
            rect_beam(dark, (weight_x - px, weight_y - py, z),
                      (weight_x + px, weight_y + py, z),
                      HP_UPPER_HOLDER_WEIGHT_WIDTH, 0.012)
        return {MAT_DARK: dark, MAT_STRUCTURE: steel}

    # The lower blade has its own compact holder and coupling lug, but no
    # second pair of weights hanging from the blade shaft.  The crossed
    # equalising levers seen farther down the mast are a separate assembly.
    extrude_xy(dark, [(-0.072, 0.048), (0.058, 0.040),
                      (0.082, -0.032), (-0.026, -0.096)],
               rear_z - 0.032, rear_z + 0.016)
    cylinder(dark, (0.0, 0.0, rear_z - 0.010), 0.041, 0.062,
             sides=(24, 16, 10)[lod])
    axle = steel if lod == 0 else dark
    cylinder(axle, (0.0, 0.0, rear_z - 0.046), 0.016, 0.010,
             sides=(18, 12, 8)[lod])
    rect_beam(dark, (-0.016, -0.030, rear_z - 0.038),
              (-0.112, -0.096, rear_z - 0.050), 0.027, 0.016)
    cylinder(dark, (-0.112, -0.096, rear_z - 0.050),
             0.023, 0.024, sides=(16, 10, 8)[lod])
    # The lower blade parks vertically in Hp 0, so its shaft-mounted balance
    # must start in the same quarter-turned pose as that blade.
    for prim in (dark, steel):
        rotate_prim_z(prim, math.pi / 2.0)
    return {MAT_DARK: dark, MAT_STRUCTURE: steel}


def hp_equalizer_mesh(second=False, lod=0):
    """One hammer-shaped Hp equalising lever on the mid-mast shaft.

    Two-arm un-coupled signals carry two independently moving levers at
    slightly different rear depth planes.  They are deliberately not part of
    either blade-holder mesh: the long vertical rods connect these levers to
    the blade shafts, and their bearing stays fixed at half mast height.
    """
    dark, steel = Prim(), Prim()
    direction = 1.0 if second else -1.0
    z = -0.286 - (0.035 if second else 0.0)
    end_x = direction * HP_EQUALIZER_REACH
    end_y = -HP_EQUALIZER_DROP

    # Forged flat lever with a round eye at the common axle.
    cylinder(dark, (0.0, 0.0, z), 0.044, 0.022,
             sides=(24, 16, 10)[lod])
    rect_beam(dark, (direction * 0.026, -0.012, z),
              (end_x, end_y, z), 0.034, 0.016)
    if lod < 2:
        annulus(dark, (0.0, 0.0), 0.046, 0.023,
                z - 0.014, z + 0.014, sides=(24, 16)[lod])

    # The weight is a short hammer head transverse to the lever and visibly
    # laminated in the prototype.  Keep three layers only at LOD0; the lower
    # LODs retain the same silhouette as a single block.
    length = math.hypot(end_x, end_y)
    px = -end_y / length * HP_EQUALIZER_WEIGHT_SPAN * 0.5
    py = end_x / length * HP_EQUALIZER_WEIGHT_SPAN * 0.5
    layers = 3 if lod == 0 else 1
    for layer in range(layers):
        layer_z = z - (layer - (layers - 1) * 0.5) * 0.013
        rect_beam(dark, (end_x - px, end_y - py, layer_z),
                  (end_x + px, end_y + py, layer_z),
                  HP_EQUALIZER_WEIGHT_WIDTH, 0.011)
    cylinder(steel if lod == 0 else dark,
             (0.0, 0.0, z - 0.020), 0.016, 0.012,
             sides=(18, 12, 8)[lod])
    return {MAT_DARK: dark, MAT_STRUCTURE: steel}


def hp_selector_holes(lower=False):
    """Return red and alternate glass centres around the selector bearing.

    Hp 0 shows the red glass in front of the one fixed lamp.  On clearing the
    signal the upper selector turns anticlockwise; the lower selector turns
    clockwise for Hp 2.  Therefore the parked alternate panes are mirrored,
    but both land at precisely the same optical axis after their 60° travel.
    """
    red = (HP_SELECTOR_RADIUS, 0.0)
    angle = math.radians(HP_SELECTOR_SWING_DEGREES * (1.0 if lower else -1.0))
    alternate = (HP_SELECTOR_RADIUS * math.cos(angle),
                 HP_SELECTOR_RADIUS * math.sin(angle))
    return red, alternate


def hp_spectacle_mesh(lower=False, lod=0):
    """Compact, independently pivoted two-glass Hauptsignal selector."""
    steel, dark, red_filter, other_filter = Prim(), Prim(), Prim(), Prim()
    red_hole, other_hole = hp_selector_holes(lower)
    # Fig. 107 shows a thin pear-shaped carrier around the bearing and both
    # glasses, not two separate lantern cases and not a plate spanning most of
    # the signal head.  Construct the carrier from the exact optical centres;
    # the slightly smaller bearing boss gives the characteristic narrow neck.
    sides = (48, 28, 14)[lod]
    outline_samples = (36, 24, 12)[lod]
    outline = []
    for (cx, cy), radius in (
            ((0.0, 0.0), 0.050),
            (red_hole, HP_SELECTOR_RING_OUTER_RADIUS),
            (other_hole, HP_SELECTOR_RING_OUTER_RADIUS)):
        outline.extend([
            (cx + radius * math.cos(2.0 * math.pi * i / outline_samples),
             cy + radius * math.sin(2.0 * math.pi * i / outline_samples))
            for i in range(outline_samples)
        ])
    extrude_xy(dark, convex_hull_xy(outline), -0.140, -0.128)
    for centre in (red_hole, other_hole):
        annulus(dark, centre, HP_SELECTOR_RING_OUTER_RADIUS,
                HP_SELECTOR_RING_INNER_RADIUS, -0.136, -0.102,
                sides=max(16, sides))
    glass_radius = LANTERN_GLASS_DIAMETER * 0.5
    optical_lens(red_filter, red_hole, glass_radius, -0.105,
                 sides=sides, rings=(6, 4, 3)[lod],
                 edge_depth=0.0015, crown=0.009, fresnel_step=0.00025,
                 centre_depth=0.011)
    optical_lens(other_filter, other_hole, glass_radius, -0.105,
                 sides=sides, rings=(6, 4, 3)[lod],
                 edge_depth=0.0015, crown=0.009, fresnel_step=0.00025,
                 centre_depth=0.011)
    cylinder(dark, (0.0, 0.0, -0.137), 0.050, 0.032,
             sides=(28, 18, 10)[lod])
    cylinder(steel, (0.0, 0.0, -0.157), 0.023, 0.014,
             sides=(20, 12, 8)[lod])
    if lod < 2:
        # Short slotted coupling lug; the long hook lever itself is fixed to
        # the lantern slide and is modelled with the static rear mechanism.
        beam(dark, (0.0, 0.0, -0.161),
             (-0.090 if lower else 0.090, -0.105, -0.176),
             0.010, max(6, 9 - lod * 2))
    return {MAT_STRUCTURE: steel, MAT_DARK: dark,
            "ruby filter glass": red_filter,
            ("amber filter glass" if lower else "green filter glass"): other_filter}


def hp_lamp_offset(lower=False):
    # The fixed lamp is level with the red glass in Hp 0.  Its position is
    # expressed relative to the blade shaft, while the selector bearing sits
    # HP_SELECTOR_AXIS_DROP lower on the lantern slide.
    red_hole, _alternate = hp_selector_holes(lower)
    return (red_hole[0], red_hole[1] - HP_SELECTOR_AXIS_DROP)


def lantern(prims, y, lower=False, lod=0):
    # Compact Siemens-style electric lamp behind the moving spectacle.  The
    # square rear cover, circular service lid, rain cap, glands and one optical
    # barrel are all visible in the close rear reference.
    dx, dy = hp_lamp_offset(lower)
    x, ly = dx, y + dy
    chamfered_box_xy(prims["black"], (x, ly), 0.276, 0.292,
                     0.020, 0.245, 0.025)
    # Folded zinc-coated rear cover.  This is a separate construction behind
    # the black optical barrel: the prototype's rear elevation is a bright,
    # shallow square tray with side cheeks and a round service lid, not a
    # black box with a green-looking ring.
    if lod < 2:
        chamfered_box_xy(prims["galvanised"], (x, ly), 0.252, 0.270,
                         -0.031, 0.026, 0.014)
        # Shallow folded side cheeks and rain lip remain visible in rear and
        # profile views.  They sit wholly behind the black front housing, so a
        # rear-detail correction cannot alter the train-facing silhouette.
        prims["galvanised"].box((x - 0.151, ly - 0.126, -0.052),
                                (x - 0.126, ly + 0.132, 0.018))
        prims["galvanised"].box((x + 0.126, ly - 0.126, -0.052),
                                (x + 0.151, ly + 0.132, 0.018))
        prims["galvanised"].box((x - 0.137, ly + 0.126, -0.052),
                                (x + 0.137, ly + 0.154, 0.018))

        # Dark sealing gasket, pressed circular lid and its shallow embossed
        # centre.  Ordering in Z is intentional: negative Z is the rear side.
        annulus(prims["dark"], (x, ly), 0.108, 0.100,
                -0.047, -0.030, sides=(40, 24)[lod])
        annulus(prims["galvanised"], (x, ly), 0.100, 0.086,
                -0.057, -0.038, sides=(40, 24)[lod])
        cylinder(prims["galvanised"], (x, ly, -0.055), 0.086, 0.018,
                 sides=(40, 24)[lod])
        annulus(prims["dark"], (x, ly), 0.069, 0.066,
                -0.066, -0.063, sides=(36, 20)[lod])

        # Four restrained cover fasteners visible in the service photograph.
        # They are neutral blackened hardware, never orange decoration.
        for sx, sy in ((-1, -1), (1, -1), (-1, 1), (1, 1)):
            cylinder(prims["dark"],
                     (x + sx * 0.086, ly + sy * 0.091, -0.058),
                     0.008, 0.010, sides=8)

        # Two cable glands are mounted directly on the round service lid.  The
        # complete black leads live in ``mast_rods`` so they can run all the
        # way to the foot terminal; keeping short stubs here was exactly what
        # made them look severed in the simulator.
        for gx in (x - HP_LANTERN_LEAD_GLAND_OFFSET,
                   x + HP_LANTERN_LEAD_GLAND_OFFSET):
            prims["galvanised"].box((gx - 0.018, ly - 0.020, -0.078),
                                    (gx + 0.018, ly + 0.018, -0.064))
            cylinder(prims["dark"], (gx, ly - 0.036, -0.073),
                     0.012, 0.032, axis=(0, 1, 0),
                     sides=(10, 8)[lod])
    # One fixed forward optical barrel.  Colour selection happens solely in
    # the mechanical two-glass carrier in front of it.
    annulus(prims["dark"], (x, ly), 0.108, 0.086,
            0.245, 0.326, sides=(44, 26, 14)[lod])
    prims["black"].box((x - 0.154, ly + 0.126, 0.005),
                       (x + 0.154, ly + 0.164, 0.275))
    prims["dark"].box((x - 0.134, ly + 0.160, 0.020),
                      (x + 0.134, ly + 0.178, 0.235))
    # Mast bracket and hinge.
    beam(prims["steel"], (0.055, ly + 0.02, 0.080),
         (x - 0.138, ly + 0.02, 0.080), 0.011,
         max(6, 9 - lod * 2))
    cylinder(prims["dark"], (x - 0.148, ly, 0.115), 0.017, 0.038,
             axis=(1, 0, 0), sides=(12, 8, 6)[lod])


def lit_lens(y, material_name, lower=False, lod=0):
    lens = Prim()
    # Small source behind the colour glass.  The former source sat in front of
    # the selector and appeared as a large, opaque glowing semicircle.
    dx, dy = hp_lamp_offset(lower)
    optical_lens(lens, (dx, y + dy), LANTERN_GLASS_DIAMETER * 0.36,
                 0.344, sides=(48, 28, 14)[lod], rings=(6, 4, 3)[lod],
                 edge_depth=0.0005, crown=0.0035,
                 fresnel_step=0.00008, centre_depth=0.0045)
    return {material_name: lens}


def base_prims():
    return {name: Prim() for name in ("concrete", "steel", "galvanised", "dark", "red", "white",
                                      "black", "yellow", "rust", "grime", "ne2",
                                      "filter_green", "filter_amber",
                                      "led_filter_green", "led_filter_amber")}


def mapped(prims):
    names = {"concrete": MAT_CONCRETE, "steel": MAT_STRUCTURE,
             "galvanised": MAT_GALVANISED,
             "dark": MAT_DARK, "red": MAT_RED, "white": MAT_WHITE,
             "black": MAT_BLACK, "yellow": MAT_YELLOW,
             "rust": MAT_RUST, "grime": MAT_GRIME}
    names["ne2"] = MAT_NE2
    names["filter_green"] = "green filter glass"
    names["filter_amber"] = "amber filter glass"
    names["led_filter_green"] = MAT_LED_GREEN_FILTER
    names["led_filter_amber"] = MAT_LED_AMBER_FILTER
    return {names[key]: value for key, value in prims.items() if value.pos}


def hp_lantern_mesh(y, lower=False, lod=0):
    """One fixed Hp lamp body, kept separate for component-level review."""
    prims = base_prims()
    lantern(prims, y, lower=lower, lod=lod)
    return mapped(prims)


def hp_head_pulley_wheel(prims, height, mast, lod):
    """Add the upper cable wheel with its axle on the front/rear Z axis."""
    centre = hp_head_pulley_centre(height, mast)
    cylinder(prims["dark"], centre, HP_HEAD_PULLEY_RADIUS,
             HP_HEAD_PULLEY_DEPTH, axis=(0, 0, 1),
             sides=(32, 18, 10)[lod])
    if lod < 2:
        cylinder(prims["steel"], centre, 0.025,
                 HP_HEAD_PULLEY_DEPTH + 0.013, axis=(0, 0, 1),
                 sides=(18, 12)[lod])


def hp_head_pulley(prims, height, mast, lod):
    """Add the common wheel and only the holder-specific weather protection."""
    hp_head_pulley_wheel(prims, height, mast, lod)
    if lod < 2 and mast == "schmal":
        # The Schmalmast sheet is folded into a small roof with a side notch.
        # Only a close-fitting upper guard surrounds the wheel; the former
        # full rectangular block made it look like a different winch type.
        wheel_x, wheel_y, wheel_z = hp_head_pulley_centre(height, mast)
        half_depth = HP_HEAD_PULLEY_DEPTH * 0.5
        annulus_arc(
            prims["dark"], (wheel_x, wheel_y),
            HP_HEAD_PULLEY_RADIUS + 0.022,
            HP_HEAD_PULLEY_RADIUS + 0.007,
            wheel_z - half_depth - 0.014,
            wheel_z + half_depth + 0.014,
            0.0, math.pi, segments=(16, 10)[lod],
        )


def hp_end_drive_disc(prims, lod):
    """Add the offset grooved end-drive disc, without its external linkage."""
    cx, cy, cz = HP_END_DRIVE_CENTRE
    sides = (48, 26, 14)[lod]
    cylinder(prims["dark"], (cx, cy, cz), HP_END_DRIVE_RADIUS,
             HP_END_DRIVE_DEPTH, sides=sides)

    # Figs. 108/109 show a broad outer drive rim and nested control grooves,
    # not a featureless black coin.  Shallow raised rings retain that reading
    # without pretending the unavailable groove profile is a measured part.
    face_z0 = cz - HP_END_DRIVE_DEPTH * 0.5 - 0.006
    face_z1 = face_z0 + 0.009
    annulus(prims["steel"], (cx, cy), HP_END_DRIVE_RADIUS * 0.94,
            HP_END_DRIVE_RADIUS * 0.82, face_z0, face_z1,
            sides=max(12, sides))
    if lod < 2:
        annulus(prims["dark"], (cx, cy), HP_END_DRIVE_RADIUS * 0.74,
                HP_END_DRIVE_RADIUS * 0.67, face_z0 - 0.003, face_z1 + 0.003,
                sides=sides)
    cylinder(prims["steel"], (cx, cy, face_z0 - 0.006), 0.037, 0.020,
             sides=(24, 14, 10)[lod])


def hp_end_drive(prims, arms, lod):
    """Add the end drive, its frame, angle lever(s), crank and rod joints."""
    hp_end_drive_disc(prims, lod)
    cx, cy, cz = HP_END_DRIVE_CENTRE
    rear_face = cz - HP_END_DRIVE_DEPTH * 0.5 - 0.020
    upper_joint = (cx + 0.120, cy + 0.300, rear_face)

    # Triangular bearing frame between the mast foot and the upper drive axle.
    # This replaces the fictitious freestanding rectangular cage.
    rect_beam(prims["dark"], (0.015, cy + 0.360, rear_face),
              upper_joint, 0.034, 0.016)
    rect_beam(prims["dark"], (0.015, cy + 0.155, rear_face),
              upper_joint, 0.034, 0.016)
    cylinder(prims["steel"], upper_joint, 0.026, 0.030,
             sides=(20, 12, 8)[lod])

    # Rear Winkelhebel and short link to the first exposed vertical rod.
    rect_beam(prims["dark"], (cx, cy, rear_face - 0.010),
              (upper_joint[0], upper_joint[1], rear_face - 0.010),
              0.052, 0.018)
    rect_beam(prims["dark"], upper_joint,
              (0.360, cy + 0.385, rear_face), 0.030, 0.014)
    cylinder(prims["steel"], (cx, cy, rear_face - 0.020), 0.031, 0.024,
             sides=(20, 12, 8)[lod])

    # A two-wing end drive carries a second angle lever on the opposite face
    # of the disc, coupled to the second rod rather than sharing one rigid bar.
    if arms == 2 and lod < 2:
        front_face = cz + HP_END_DRIVE_DEPTH * 0.5 + 0.018
        second_joint = (cx + 0.096, cy + 0.270, front_face)
        rect_beam(prims["dark"], (cx, cy, front_face), second_joint,
                  0.044, 0.016)
        rect_beam(prims["dark"], second_joint,
                  (0.435, cy + 0.405, front_face), 0.027, 0.013)
        cylinder(prims["steel"], second_joint, 0.022, 0.026,
                 sides=(18, 12)[lod])

    if lod == 0:
        # Fold-down hand crank on the outer cheek and small ratchet pawl.
        crank_z = rear_face - 0.032
        beam(prims["steel"], (cx, cy, crank_z),
             (cx + 0.018, cy - 0.135, crank_z - 0.018), 0.009, 10)
        beam(prims["dark"], (cx + 0.018, cy - 0.135, crank_z - 0.018),
             (cx + 0.105, cy - 0.135, crank_z - 0.018), 0.012, 10)
        rect_beam(prims["dark"], (cx - 0.020, cy + 0.165, rear_face),
                  (cx + 0.025, cy + 0.215, rear_face), 0.020, 0.010)


def main_static_components(nominal_height, mast, arms, scheme, lod):
    """Build separately reviewable fixed Hp assemblies, excluding lamps.

    The old exporter put every fixed part into one ``mast`` mesh.  That made a
    local edit impossible to protect: a change intended for the lattice could
    also alter the recognition boards, mast head or end drive without tripping
    a component guard.  Keep the exact same geometry and material recipes, but
    emit one mesh per real functional assembly so the workbench can lock the
    unaffected parts.
    """
    height, lower_y = hp_pivot_levels(nominal_height)
    components = {}

    foundation_prims = base_prims()
    foundation(foundation_prims)
    components["mast_foundation"] = foundation_prims

    structure_prims = base_prims()
    lattice_mast(structure_prims, height, narrow=mast == "schmal",
                 scheme=scheme, lod=lod)
    components["mast_structure"] = structure_prims

    if scheme != "altanstrich":
        board_prims = base_prims()
        mast_board(board_prims, height, mast, lod)
        components["mast_board"] = board_prims
    upper_y = height

    # Through-shafts, cast cheeks, rocking levers and the separate rear rods.
    # These are the parts that distinguish a real rear elevation from a mirrored
    # signal face.
    head_prims = base_prims()
    levels = [upper_y] if arms == 1 else [upper_y, lower_y]
    for y in levels:
        cylinder(head_prims["dark"], (0.0, y, 0.305), 0.059, 0.300,
                 sides=(32, 20, 12)[lod])
        cylinder(head_prims["dark"], (0.0, y, -0.202), 0.074, 0.044,
                 sides=(32, 20, 12)[lod])
        cylinder(head_prims["steel"], (0.0, y, -0.231), 0.025, 0.018,
                 sides=(20, 12, 8)[lod])
        if lod == 0:
            # Continuous return spring between selector and mast.  Both blade
            # levels use the same side: alternating the sign put the upper
            # spring outside the mechanism.  Short hooks terminate in visible
            # fixed eyes, so neither end floats when viewed from either side.
            spring_y0 = y - HP_RETURN_SPRING_TOP_DROP
            spring_y1 = y - HP_RETURN_SPRING_BOTTOM_DROP
            spring_start, spring_end = coil_spring_y(
                head_prims["dark"],
                (HP_RETURN_SPRING_X, HP_RETURN_SPRING_Z),
                spring_y0, spring_y1,
                HP_RETURN_SPRING_RADIUS, HP_RETURN_SPRING_TURNS,
                HP_RETURN_SPRING_WIRE_RADIUS,
                segments_per_turn=8,
            )
            upper_eye = (0.050, spring_y0 + 0.030, -0.190)
            lower_eye = (0.050, spring_y1 - 0.030, -0.190)
            beam(head_prims["dark"], upper_eye, spring_start, 0.0055, 7)
            beam(head_prims["dark"], spring_end, lower_eye, 0.0055, 7)
            for eye in (upper_eye, lower_eye):
                cylinder(head_prims["steel"], eye, 0.013, 0.014,
                         axis=(0, 0, 1), sides=12)

    # Construction-specific mast head around one common lantern-hoist wheel.
    # The real Gittermast continues as an open four-angle cage.  The Schmalmast
    # instead ends in the compact folded roof documented by its assembly sheet.
    # Reversing these silhouettes was the reason the two variants appeared to
    # carry unrelated winches in the old rear view.
    head_layout = hp_head_layout(mast)
    head_above = head_layout["above_pivot"]
    head_y0, head_y1 = height - 0.105, height + head_above
    if mast == "schmal":
        half_width = HP_SCHMAL_MAST_WIDTH * 0.5
        half_depth = HP_SCHMAL_MAST_DEPTH * 0.5
        plate = HP_SCHMAL_MAST_PLATE
        # Two narrow folded faces read as the solid 100-mm head in front/rear
        # elevation; the side remains hollow and shows the mast depth.  A top
        # roof and two sparse diaphragms hold the faces together.
        for z in (-half_depth + plate * 0.5,
                  half_depth - plate * 0.5):
            head_prims["steel"].box(
                (-half_width, head_y0, z - plate * 0.5),
                (half_width, head_y1 - 0.030, z + plate * 0.5),
            )
        for cy, strap_h in ((head_y0 + 0.035, 0.050),
                            (height + 0.145, 0.040)):
            head_prims["steel"].box(
                (-half_width, cy - strap_h * 0.5, -half_depth),
                (half_width, cy + strap_h * 0.5, half_depth),
            )
        head_prims["steel"].box(
            (-half_width - 0.010, head_y1 - 0.040, -half_depth - 0.010),
            (half_width + 0.010, head_y1, half_depth + 0.010),
        )

        # Short doubled cheek from the folded side notch to the common axle.
        wheel_x, wheel_y, wheel_z = hp_head_pulley_centre(height, mast)
        for z in (wheel_z - HP_HEAD_PULLEY_DEPTH * 0.5 - 0.010,
                  wheel_z + HP_HEAD_PULLEY_DEPTH * 0.5 + 0.010):
            rect_beam(head_prims["steel"],
                      (half_width, wheel_y, z),
                      (wheel_x, wheel_y, z), 0.036, 0.012)
    else:
        cap_half = head_layout["support_half_width"]
        head_z_front, head_z_rear = 0.115, -0.115
        chord_width, chord_depth = 0.030, 0.024
        for x in (-cap_half, cap_half):
            for z in (head_z_rear, head_z_front):
                rect_beam(head_prims["steel"],
                          (x, head_y0, z), (x, head_y1, z),
                          chord_width, chord_depth)
        # Open horizontal frames retain the rectangular mast-tip window from
        # both front and side views.  No continuous plate is allowed here.
        for cy in (head_y0 + 0.035, height + 0.145, head_y1 - 0.020):
            for z in (head_z_rear, head_z_front):
                rect_beam(head_prims["steel"],
                          (-cap_half, cy, z), (cap_half, cy, z),
                          0.030, 0.018)
            for x in (-cap_half, cap_half):
                rect_beam(head_prims["steel"],
                          (x, cy, head_z_rear),
                          (x, cy, head_z_front), 0.030, 0.018)

    # The common pulley lies in the same X/Y plane as the signal face.  Its
    # construction-specific brackets were emitted above.
    hp_head_pulley(head_prims, height, mast, lod)
    components["mast_head"] = head_prims

    # Rear mechanism: one continuous double cable for the lantern lift plus
    # positively connected blade operating rods.  Every visible endpoint now
    # enters a wheel, joint, shaft shoe or the mast foot; no line terminates in
    # open air.
    rod_prims = base_prims()
    cable_sides = max(5, 8 - lod * 2)
    cable_runs = hp_head_cable_runs(height, mast)
    for cable_top, cable_bottom in cable_runs:
        beam(rod_prims["dark"], cable_bottom, cable_top,
             HP_HEAD_CABLE_RADIUS, cable_sides)
    if lod < 2:
        # The separate electrical pair starts exactly at the underside of the
        # two Siemens lid glands and terminates in a closed foot box.  On a
        # two-arm signal the aligned lower glands sit on these same runs.
        lead_runs = hp_lantern_lead_paths(nominal_height)
        for path in lead_runs:
            for start, end in zip(path, path[1:]):
                beam(rod_prims["dark"], start, end,
                     HP_LANTERN_LEAD_RADIUS, cable_sides)
        terminal_y0 = 0.125
        chamfered_box_xy(
            rod_prims["dark"],
            (HP_LANTERN_TERMINAL_X,
             (terminal_y0 + HP_LANTERN_LEAD_BOTTOM) * 0.5),
            0.110, HP_LANTERN_LEAD_BOTTOM - terminal_y0,
            HP_LANTERN_LEAD_Z - 0.030,
            HP_LANTERN_LEAD_Z + 0.018,
            0.012,
        )
        for path in lead_runs:
            cylinder(rod_prims["galvanised"], path[-1], 0.010, 0.026,
                     axis=(0, 1, 0), sides=(10, 8)[lod])
    if lod == 0:
        left_run, right_run = cable_runs
        # Guide plates clamp both legs to the rear of the mast.
        for guide_y in (1.55, 3.65, min(height - 0.55, 5.75)):
            if guide_y < height - 0.30:
                rod_prims["dark"].box(
                    (left_run[0][0] - 0.024, guide_y - 0.010,
                     HP_HEAD_CABLE_Z - 0.010),
                    (right_run[0][0] + 0.024, guide_y + 0.010,
                     HP_HEAD_CABLE_Z + 0.010),
                )
        # The lantern slide is clamped to the outer leg immediately below the
        # upper lantern body rather than being merely adjacent to the cable.
        lamp_x, _lamp_dy = hp_lamp_offset(False)
        clamp_y = height - 0.105
        rect_beam(rod_prims["steel"],
                  (right_run[0][0], clamp_y, HP_HEAD_CABLE_Z),
                  (lamp_x - 0.120, clamp_y, HP_HEAD_CABLE_Z),
                  0.022, 0.012)

    rod_paths = hp_operating_rod_paths(nominal_height, arms)
    for index, path in enumerate(rod_paths):
        for start, end in zip(path, path[1:]):
            beam(rod_prims["dark"], start, end, 0.007,
                 max(5, 8 - lod * 2))
        # Pinned shoes make the two real attachment points legible from behind.
        for joint in (path[0], path[-1]):
            cylinder(rod_prims["steel"], joint, 0.020, 0.018,
                     axis=(0, 0, 1), sides=(16, 10, 8)[lod])
        if lod == 0:
            x = path[1][0]
            z = path[1][2]
            for guide_y in (1.55, 3.55, min(path[2][1] - 0.25, 5.55)):
                if path[1][1] + 0.15 < guide_y < path[2][1] - 0.10:
                    rect_beam(rod_prims["dark"],
                              (0.135, guide_y, z),
                              (x, guide_y, z), 0.018, 0.010)
                    rod_prims["steel"].box(
                        (x - 0.016, guide_y - 0.026, z - 0.013),
                        (x + 0.016, guide_y + 0.026, z + 0.013),
                    )
    components["mast_rods"] = rod_prims

    drive_prims = base_prims()
    hp_end_drive(drive_prims, arms, lod)
    components["mast_drive"] = drive_prims

    return {stem: mapped(prims) for stem, prims in components.items()}


def build_main(height, mast, arms, shortened=False, scheme="db_gruen", negative=False):
    upper_y, lower_y = hp_pivot_levels(height)
    mesh_specs, nodes = [], []
    for lod, _distance in HP_LODS:
        suffix = f"_LOD{lod}"
        for stem, mesh in main_static_components(
                height, mast, arms, scheme, lod).items():
            name = f"{stem}{suffix}"
            mesh_specs.append((name, mesh))
            nodes.append({"name": name, "mesh": name})

        moving = [
            ("fluegel1", main_arm_mesh(False, shortened, negative, lod), upper_y),
            ("blende1", hp_spectacle_mesh(False, lod),
             upper_y - HP_SELECTOR_AXIS_DROP),
        ]
        if arms == 2:
            moving += [
                ("fluegel2", main_arm_mesh(True, shortened, negative, lod), lower_y),
                ("blende2", hp_spectacle_mesh(True, lod),
                 lower_y - HP_SELECTOR_AXIS_DROP),
            ]
        for stem, mesh, pivot_y in moving:
            name = f"{stem}{suffix}"
            mesh_specs.append((name, mesh))
            nodes.append({"name": name, "mesh": name,
                          "translation": [0, pivot_y, 0.47]})

        balances = [("gewicht1", hp_balance_mesh(False, lod), upper_y)]
        if arms == 2:
            balances.append(("gewicht2", hp_balance_mesh(True, lod), lower_y))
        for stem, mesh, pivot_y in balances:
            name = f"{stem}{suffix}"
            mesh_specs.append((name, mesh))
            nodes.append({"name": name, "mesh": name,
                          "translation": [0, pivot_y, 0.0]})

        equalizer_y = height * HP_EQUALIZER_HEIGHT_FACTOR
        equalizers = [
            ("gewicht_ausgleich1", hp_equalizer_mesh(False, lod)),
        ]
        if arms == 2:
            equalizers.append(
                ("gewicht_ausgleich2", hp_equalizer_mesh(True, lod)))
        for stem, mesh in equalizers:
            name = f"{stem}{suffix}"
            mesh_specs.append((name, mesh))
            nodes.append({"name": name, "mesh": name,
                          "translation": [0, equalizer_y, 0.0]})

        lanterns = [("laterne1", hp_lantern_mesh(upper_y, lod=lod))]
        if arms == 2:
            lanterns.append(("laterne2", hp_lantern_mesh(
                lower_y, lower=True, lod=lod)))
        for stem, mesh in lanterns:
            name = f"{stem}{suffix}"
            mesh_specs.append((name, mesh))
            nodes.append({"name": name, "mesh": name})

        lamps = [
            ("lamp_red", lit_lens(upper_y, "lit ruby glass", lod=lod)),
            ("lamp_green", lit_lens(upper_y, "lit green glass", lod=lod)),
        ]
        if arms == 2:
            lamps.append(("lamp_yellow", lit_lens(
                lower_y, "lit amber glass", lower=True, lod=lod)))
        for stem, mesh in lamps:
            name = f"{stem}{suffix}"
            mesh_specs.append((name, mesh))
            nodes.append({"name": name, "mesh": name})

    suffix = "_kurz" if shortened else ""
    if scheme != "db_gruen":
        suffix += f"_{scheme}"
    if negative:
        suffix += "_negativ"
    filename = f"sig_form_hp_{height:g}m_{mast}_{arms}fl{suffix}.gltf"
    write_gltf(filename, filename[:-5], materials_for(scheme), mesh_specs, nodes,
               external_textures=True)
    return filename


def ne2_board(prims, bottom, size="hoch", x=0.0, lod=0):
    """DB S 525.1 Ne 2 board; its entire face is one normal-mapped texture."""
    width, height = NE2_SIZES[size]
    x0, x1, y0, y1 = x - width * 0.5, x + width * 0.5, bottom, bottom + height
    # Black-painted folded backing and two rear clamps remain geometry.  The
    # border and double chevron are deliberately absent from the mesh.
    if lod < 2:
        prims["black"].box((x0 - 0.012, y0 - 0.012, 0.200),
                           (x1 + 0.012, y1 + 0.012, 0.235))
    z = 0.238
    face = prims["ne2"]
    face.tri((x0, y0, z), (x1, y0, z), (x1, y1, z),
             ((0.0, 0.0), (1.0, 0.0), (1.0, 1.0)))
    face.tri((x0, y0, z), (x1, y1, z), (x0, y1, z),
             ((0.0, 0.0), (1.0, 1.0), (0.0, 1.0)))
    if lod == 0:
        for y in (y0 + height * 0.22, y0 + height * 0.78):
            prims["steel"].box((x0 + width * 0.15, y - 0.018, 0.165),
                               (x1 - width * 0.15, y + 0.018, 0.200))


def vr_disc_mesh(lod=0):
    white, black, yellow, rust = Prim(), Prim(), Prim(), Prim()
    # S 090.25.2 fixes the finished diameter at exactly 1000 mm.  128 sides at
    # LOD0 keep its one-metre silhouette visually circular even in close-up.
    sides = (128, 48, 20)[lod]
    radius = VR_DISC_DIAMETER * 0.5
    # The signal face is a thin sheet/enamel assembly, not a layered drum.
    # Including its shallow rear ribs the complete profile stays near 55 mm.
    cylinder(black, (0, 0, 0), radius, 0.024, sides=sides)
    cylinder(white, (0, 0, 0.012), VR_DISC_WHITE_RADIUS, 0.010, sides=sides)
    cylinder(black, (0, 0, 0.019), VR_DISC_BLACK_RING_RADIUS, 0.008,
             sides=sides)
    cylinder(yellow, (0, 0, 0.026), VR_DISC_FACE_RADIUS, 0.006, sides=sides)
    if lod < 2:
        # Rear of the pressed sheet: circumferential bead plus the pair of
        # vertical hat-section stiffeners visible on surviving Einheit discs.
        # The previous diagonal X was not present on the prototype.
        annulus(black, (0.0, 0.0), 0.455, 0.415,
                -0.032, -0.010, sides=max(24, sides // 2))
        for x in (-0.095, 0.095):
            black.box((x - 0.027, -0.425, -0.043),
                      (x + 0.027, 0.425, -0.015))
        black.box((-0.145, -0.028, -0.047),
                  (0.145, 0.028, -0.016))
        cylinder(black, (0.0, 0.0, -0.048), 0.055, 0.024,
                 sides=(32, 16)[lod])
        # Bolted lower lugs and the oblique folded operating link belong to the
        # moving disc itself; the vertical pull rods remain on the fixed mast.
        for x in (-0.095, 0.095):
            cylinder(black, (x, -0.338, -0.052), 0.018, 0.014,
                     sides=(14, 8)[lod])
        rect_beam(black, VR_DISC_CRANK_ROOT,
                  VR_DISC_CRANK_PIN, 0.032, 0.014)
        cylinder(black, VR_DISC_CRANK_PIN, 0.027, 0.020,
                 sides=(16, 10)[lod])
    return {MAT_WHITE: white, MAT_BLACK: black,
            MAT_VR_ORANGE: yellow, MAT_RUST: rust}


def vr_wing_mesh(lod=0):
    white, black, yellow = Prim(), Prim(), Prim()
    hardware, rust = Prim(), Prim()

    def about_pivot(points):
        return [(x, y + VR_WING_PIVOT_FROM_TOP) for x, y in points]

    # The visible fastener in the middle is the wing axle. Keep the rest-pose
    # outline in the same place, but centre the local geometry on this axle so
    # Vr 2 rotates around the prototype pivot instead of hinging at the top.
    outside = about_pivot([
        (-VR_WING_WIDTH * 0.5, 0.0),
        (VR_WING_WIDTH * 0.5, 0.0),
        (0.112, -1.166),
        (0.0, -VR_WING_LENGTH),
        (-0.112, -1.166),
    ])
    dark_inset = about_pivot([
        (-0.098, -0.028), (0.098, -0.028), (0.091, -1.128),
        (0.0, -1.335), (-0.091, -1.128),
    ])
    core = about_pivot([
        (-0.064, -0.047), (0.064, -0.047), (0.060, -1.090),
        (0.0, -1.250), (-0.060, -1.090),
    ])
    # Folded edge depth follows the slim framed blade in the side reference.
    # The structural sheet and all rear/edge faces are black. White enamel is
    # a thin front layer only; using it for the complete extrusion previously
    # produced a conspicuously white rear elevation.
    extrude_xy(black, outside, 0.000, 0.024)
    extrude_xy(white, outside, 0.025, 0.031)
    extrude_xy(black, dark_inset, 0.032, 0.039)
    extrude_xy(yellow, core, 0.040, 0.047)
    # Rear bearing boss plus the small front washer and bolt seen on the
    # reference. The face hardware is dark steel, never the former rust-orange
    # decorative stud.
    cylinder(black, (0, 0, -0.012), 0.060, 0.045,
             sides=(40, 24, 12)[lod])
    cylinder(black, (0, 0, 0.054), 0.032, 0.012,
             sides=6)
    cylinder(hardware, (0, 0, 0.064), 0.015, 0.010,
             sides=6)
    if lod < 2:
        black.box((-0.082, -1.166 + VR_WING_PIVOT_FROM_TOP, -0.018),
                  (0.082, -0.094 + VR_WING_PIVOT_FROM_TOP, -0.003))
        black.box((-0.105, -0.178 + VR_WING_PIVOT_FROM_TOP, -0.022),
                  (0.105, -0.141 + VR_WING_PIVOT_FROM_TOP, 0.000))
    if lod == 0:
        # Paired retaining screws through the white folded border.  The heads
        # are deliberately only 12 mm across and sit clear of the orange
        # enamel field; close prototype photographs show them, while the large
        # orange studs from an earlier draft did not exist.
        for distance in VR_WING_EDGE_FASTENERS_FROM_TOP:
            y = VR_WING_PIVOT_FROM_TOP - distance
            for x in (-0.109, 0.109):
                cylinder(hardware, (x, y, 0.052), 0.006, 0.008,
                         sides=8)
        for y in (-0.357, -0.714, -1.071):
            cylinder(black, (0.0, y + VR_WING_PIVOT_FROM_TOP, -0.040),
                     0.015, 0.015, sides=10)
    return {MAT_WHITE: white, MAT_BLACK: black,
            MAT_YELLOW: yellow, MAT_DARK: hardware, MAT_RUST: rust}


def vr_night_layout(centre_y):
    """The four physical apertures of the two double-lantern selectors."""
    half_spacing = VR_LANTERN_APERTURE_SPACING * 0.5
    # The reference front view puts the right pair about 0.80 m below the disc
    # centre and the left pair about 0.51 m below that. Both carriers hug the
    # central wing: their inner edges remain approximately 45 mm clear of its
    # documented 240-mm maximum width.
    left = (-VR_LANTERN_LATERAL_OFFSET,
            centre_y - VR_LANTERN_LEFT_DROP)
    right = (VR_LANTERN_LATERAL_OFFSET,
             centre_y - VR_LANTERN_RIGHT_DROP)
    return {
        "left_green": (left[0], left[1] + half_spacing),
        "left_amber": (left[0], left[1] - half_spacing),
        "right_amber": (right[0], right[1] + half_spacing),
        "right_green": (right[0], right[1] - half_spacing),
        "centres": (left, right),
    }


def vr_mast_envelope(mast_style):
    """Return the visible width/depth of an existing Vorsignalmast."""
    if mast_style == "1944":
        return VR_1944_MAST_WIDTH, VR_1944_MAST_DEPTH
    if mast_style == "alt_u":
        return VR_OLD_U_MAST_WIDTH, VR_OLD_U_MAST_DEPTH
    raise ValueError(f"unknown Vr mast: {mast_style}")


def vr_electric_lighting_paths(centre_y, mast_style):
    """Return a closed electrical path from both lamps into the mast foot.

    Preserved electric conversions show flexible leads leaving the underside
    of both stationary lantern backs, meeting in a small junction enclosure
    behind the mast and continuing downwards.  Keeping the points in one
    helper makes every endpoint testable; no decorative cable is allowed to
    begin or finish in open air.
    """
    layout = vr_night_layout(centre_y)
    mast_width, mast_depth = vr_mast_envelope(mast_style)
    mast_rear_z = -mast_depth * 0.5
    conduit_z = mast_rear_z - VR_LIGHTING_CONDUIT_REAR_OFFSET
    fixed_axes = (layout["left_amber"], layout["right_amber"])
    junction = (
        VR_LIGHTING_JUNCTION_X,
        centre_y - VR_LIGHTING_JUNCTION_DROP,
        mast_rear_z - VR_LIGHTING_JUNCTION_REAR_OFFSET,
    )
    junction_half_height = 0.085
    junction_top_y = junction[1] + junction_half_height
    branches = []
    for lamp_x, lamp_y in fixed_axes:
        side = 1.0 if lamp_x > 0.0 else -1.0
        # Leave the lower rear gland, turn inwards immediately, then descend
        # in the recess behind the mast.  The earlier long vertical runs at
        # the lantern x positions were visible from the train and looked like
        # wires ending in mid-air.
        rail_x = side * min(mast_width * 0.28, 0.035)
        landing_x = side * 0.025
        mast_entry_y = lamp_y - 0.255
        points = [
            (lamp_x, lamp_y - 0.145, 0.174),
            (lamp_x - side * 0.050, lamp_y - 0.195, 0.090),
            (rail_x, mast_entry_y, conduit_z),
        ]
        if mast_entry_y > junction_top_y + 0.120:
            points.append((rail_x, junction_top_y + 0.120, conduit_z))
        points.append((landing_x, junction_top_y, junction[2]))
        branches.append(tuple(points))

    trunk = (
        (junction[0], junction[1] - junction_half_height, junction[2]),
        (0.000, 0.300, conduit_z),
        (0.000, 0.240, mast_rear_z),
    )
    return {
        "junction": junction,
        "junction_half_height": junction_half_height,
        "mast_rear_z": mast_rear_z,
        "conduit_z": conduit_z,
        "branches": tuple(branches),
        "trunk": trunk,
    }


def add_vr_night_housings(prims, centre_y, lighting, mast_style, lod):
    layout = vr_night_layout(centre_y)
    width, height = (VR_LANTERN_CASE if lighting == "gas"
                     else VR_LED_LANTERN_CASE)
    depth = 0.180 if lighting == "gas" else 0.105
    case_sides = (24, 14, 8)[lod]
    # Both the gas and electric designs have one stationary light source in
    # each lateral lantern.  The vertically elongated part seen from the front
    # is the moving two-colour spectacle, not a housing with two lamps.  In the
    # Vr 0 rest position the fixed optical axes coincide with the amber panes.
    fixed_axes = (layout["left_amber"], layout["right_amber"])
    for x, output_y in fixed_axes:
        if lighting == "gas":
            capsule_y(prims["black"], (x, output_y), width * 0.88,
                      width * 0.98, 0.205, 0.205 + depth,
                      sides=case_sides)
            annulus(prims["dark"], (x, output_y), 0.105,
                    LANTERN_GLASS_DIAMETER * 0.5, 0.205 + depth,
                    0.228 + depth, sides=(40, 24, 12)[lod])
            # Chimney and gas feed belong to the stationary lantern, not to
            # the coloured spectacle or the service-only lantern lift.
            prims["black"].box((x - 0.055, output_y + width * 0.43, 0.22),
                               (x + 0.055, output_y + width * 0.59,
                                0.34 + depth))
        else:
            # Siemens electric lamphouse: one square cast body with circular
            # sealed service lid and one replaceable bulb/LED module per side.
            # There is deliberately no second lamp behind the other filter.
            # The retrofit optical unit is black-painted in the supplied
            # operational references.  Only its removable rear service lid
            # and fasteners retain the mast/steel finish.
            chamfered_box_xy(prims["black"], (x, output_y),
                             0.245, 0.255, 0.190, 0.304, 0.022)
            # A fixed black optical collar surrounds the one real light axis
            # and reaches just ahead of the thin moving filter.  Besides
            # matching the dark prototype bezel, this prevents the source
            # from appearing as a detached dot while the selector turns.
            annulus(prims["dark"], (x, output_y), 0.098, 0.082,
                    0.304, 0.372, sides=(40, 24, 12)[lod])
            if lod < 2:
                annulus(prims["dark"], (x, output_y), 0.086, 0.073,
                        0.176, 0.190, sides=(32, 18)[lod])
                cylinder(prims["steel"], (x, output_y, 0.174),
                         0.072, 0.010, sides=(32, 18)[lod])
                prims["dark"].box((x - 0.137, output_y + 0.105, 0.183),
                                   (x + 0.137, output_y + 0.148, 0.320))
                for sx, sy in ((-1, -1), (1, -1), (-1, 1), (1, 1)):
                    cylinder(prims["dark"],
                             (x + sx * 0.098, output_y + sy * 0.101, 0.169),
                             0.008, 0.010, sides=8)

        # The slim rear mounting iron is visible between lantern and mast in
        # the supplied front/side photographs.
        side = 1.0 if x > 0.0 else -1.0
        inner_edge = x - side * width * 0.44
        beam(prims["dark"], (side * 0.055, output_y, 0.145),
             (inner_edge, output_y, 0.205), 0.010, max(5, 8 - lod))

    if lighting != "gas":
        # Two flexible branches, one closed junction box and one continuous
        # trunk replace the former detached vertical line.  The whole run is
        # behind the display plane, as in the preserved Zeitz assembly.
        wiring = vr_electric_lighting_paths(centre_y, mast_style)
        junction_x, junction_y, junction_z = wiring["junction"]
        junction_half_height = wiring["junction_half_height"]
        prims["dark"].box(
            (junction_x - 0.075, junction_y - junction_half_height,
             junction_z - 0.035),
            (junction_x + 0.075, junction_y + junction_half_height,
             junction_z + 0.035),
        )
        prims["steel"].box(
            (junction_x - 0.062, junction_y - junction_half_height + 0.012,
             junction_z - 0.043),
            (junction_x + 0.062, junction_y + junction_half_height - 0.012,
             junction_z - 0.034),
        )
        # A short rear bracket fixes the junction enclosure to the mast.  Its
        # front face touches the actual rear flange for both mast designs.
        prims["steel"].box(
            (-0.032, junction_y - 0.024, junction_z + 0.035),
            (0.032, junction_y + 0.024, wiring["mast_rear_z"]),
        )
        for path in (*wiring["branches"], wiring["trunk"]):
            for start, end in zip(path, path[1:]):
                beam(prims["dark"], start, end,
                     VR_LIGHTING_CABLE_RADIUS, max(6, 10 - 2 * lod))
            for point in path[1:-1]:
                cylinder(prims["dark"], point,
                         VR_LIGHTING_CABLE_RADIUS * 1.18,
                         VR_LIGHTING_CABLE_RADIUS * 1.8,
                         sides=max(6, 10 - 2 * lod))
        for path in wiring["branches"]:
            cylinder(prims["dark"], path[0], 0.012, 0.028,
                     axis=(0, 1, 0), sides=max(8, 14 - 2 * lod))
        # The last gland physically enters the mast rear flange instead of
        # terminating a few centimetres beside it.
        cylinder(prims["dark"], wiring["trunk"][-1], 0.012, 0.026,
                 axis=(0, 1, 0), sides=max(8, 14 - 2 * lod))
    return layout


def vr_spectacle_mesh(right, lighting, lod=0):
    """Rotating two-colour plate in front of one stationary lantern.

    Both plates start with amber on the fixed lamp axis.  A half-turn swaps in
    green.  Gas and electric versions use the same mechanical principle; only
    the electric filter glass is substantially thinner and clearer.  The
    linkage drives the two plates in opposite directions, so separate nodes
    preserve that visible counter-motion.
    """
    dark, steel, amber, green = Prim(), Prim(), Prim(), Prim()
    electric = lighting != "gas"
    width, height = VR_LED_LANTERN_CASE if electric else VR_LANTERN_CASE
    sides = (48, 28, 14)[lod]
    rings = (7, 5, 3)[lod]
    half_spacing = VR_LANTERN_APERTURE_SPACING * 0.5
    amber_y = half_spacing if right else -half_spacing
    green_y = -amber_y

    plate_depth = 0.012 if electric else 0.022
    capsule_y(dark, (0.0, 0.0), width, height, 0.000, plate_depth,
              sides=(32, 20, 10)[lod])
    for aperture_y in (amber_y, green_y):
        annulus(dark, (0.0, aperture_y), 0.105,
                LANTERN_GLASS_DIAMETER * 0.5, plate_depth - 0.002,
                plate_depth + (0.006 if electric else 0.022),
                sides=(40, 24, 12)[lod])
    lens_back = plate_depth + (0.006 if electric else 0.023)
    lens_options = ({
        "edge_depth": VR_LED_LENS_EDGE_DEPTH,
        "crown": VR_LED_LENS_CROWN,
        "fresnel_step": VR_LED_FRESNEL_STEP,
        "centre_depth": VR_LED_LENS_CENTRE_DEPTH,
    } if electric else {})
    optical_lens(amber, (0.0, amber_y), LANTERN_GLASS_DIAMETER * 0.5,
                 lens_back, sides=sides, rings=rings, **lens_options)
    optical_lens(green, (0.0, green_y), LANTERN_GLASS_DIAMETER * 0.5,
                 lens_back, sides=sides, rings=rings, **lens_options)
    # Central spindle and the short rear coupling lug remain visible in side
    # and rear views while rotating with the selector plate.
    cylinder(dark, (0.0, 0.0, -0.010), 0.035, 0.050,
             sides=(24, 16, 10)[lod])
    # The axle is flush and blackened in the supplied front photographs.  A
    # formerly bright centre pin plus four decorative corner fasteners read as
    # the spurious orange/green dots reported by the user, so no contrasting
    # front hardware is generated here.
    cylinder(dark, (0.0, 0.0, plate_depth + 0.006), 0.008, 0.006,
             sides=(16, 10, 8)[lod])
    beam(dark, (0.0, 0.0, -0.030),
         ((0.13 if right else -0.13), 0.0, -0.055),
         0.012, max(5, 8 - lod))
    amber_material = MAT_LED_AMBER_FILTER if electric else "amber filter glass"
    green_material = MAT_LED_GREEN_FILTER if electric else "green filter glass"
    return {MAT_DARK: dark, MAT_STRUCTURE: steel,
            amber_material: amber, green_material: green}


def vr_lamp_mesh(positions, colour, lod=0, gas=False):
    lens = Prim()
    depth = 0.180 if gas else 0.105
    # The deep gas filter encloses its source near the crown.  Some realtime
    # renderers cannot resolve an emissive surface behind KHR transmission, so
    # the electric source sits just ahead of the cover, but is deliberately
    # smaller: the surrounding clear-glass annulus still shows the new shallow
    # profile and Fresnel highlight while the selected aspect remains legible.
    # Bevy cannot reliably composite an emissive surface ten millimetres
    # behind two transmissive glTF layers.  The electric source therefore
    # finishes flush with the fixed aperture collar while remaining much
    # smaller than the coloured glass; the glass annulus and its Fresnel
    # highlight stay visible around it.
    z = 0.274 + depth if gas else 0.366
    source_radius = LANTERN_GLASS_DIAMETER * (0.405 if gas else 0.370)
    for x, y in positions:
        if gas:
            cylinder(lens, (x, y, z), source_radius,
                     0.003, sides=(48, 28, 14)[lod])
        else:
            optical_lens(lens, (x, y), source_radius, z - 0.001,
                         sides=(56, 32, 16)[lod], rings=(6, 4, 3)[lod],
                         edge_depth=0.0004, crown=0.0024,
                         fresnel_step=0.00005, centre_depth=0.0030)
    return {colour: lens}


def gas_cartridge_mesh(centre_y, lod=0):
    bottle, red, dark = Prim(), Prim(), Prim()
    sides = (24, 14, 8)[lod]
    # The DB propane design puts one 3.2 kg bottle in a retaining basket
    # directly under each lantern.  Both assemblies ride the service-only
    # lantern lift; they never shuttle between colours during an aspect change.
    # Preserved DB lanterns show an open round rod basket around a pale-green
    # cylinder with a tapered red shoulder.  The former three solid rectangular
    # cross-plates hid the bottle and had no counterpart on that assembly.
    layout = vr_night_layout(centre_y)
    for x, lamp_y in (layout["left_amber"], layout["right_amber"]):
        bottle_z = 0.245
        body_bottom = lamp_y - 0.505
        body_top = lamp_y - 0.195
        shoulder_top = lamp_y - 0.102
        cylinder(bottle,
                 (x, (body_bottom + body_top) * 0.5, bottle_z),
                 0.083, body_top - body_bottom,
                 axis=(0, 1, 0), sides=sides)
        frustum_y(red, (x, bottle_z), body_top, shoulder_top,
                  0.079, 0.045, sides=sides)
        cylinder(dark, (x, lamp_y - 0.080, bottle_z), 0.026, 0.044,
                 axis=(0, 1, 0), sides=max(8, sides))

        cage_bottom = lamp_y - 0.525
        cage_top = lamp_y - 0.070
        hoop_segments = (28, 18, 10)[lod]
        rod_radius = 0.007 if lod == 0 else 0.008
        for cage_y, cage_radius in (
                (cage_bottom, 0.118),
                (lamp_y - 0.300, 0.103),
                (cage_top, 0.105)):
            hoop_xz(dark, (x, cage_y, bottle_z), cage_radius,
                    rod_radius, hoop_segments)
        upright_count = (4, 3, 3)[lod]
        for index in range(upright_count):
            angle = 2.0 * math.pi * index / upright_count
            cage_x = x + 0.108 * math.cos(angle)
            cage_z = bottle_z + 0.108 * math.sin(angle)
            beam(dark, (cage_x, cage_bottom, cage_z),
                 (cage_x, cage_top, cage_z), rod_radius,
                 max(5, 8 - lod))
    return {MAT_GAS_BOTTLE: bottle, MAT_RED: red, MAT_DARK: dark}


def vr_wire_drive(prims, lod):
    """Open mechanical distant-signal drive from historical Fig. 122.

    The mechanism is intentionally its own construction module.  Earlier
    versions generated it together with an electrical lighting cabinet, which
    mixed drive technology and night-sign technology and made visual fixes to
    one variant damage the other.
    """
    # The narrow mechanism sits behind/left of the mast. Its two wheel cases
    # are seen nearly edge-on from the train, but read as round pulleys from
    # the side, as on the prototype.
    for z0, z1 in ((-0.410, -0.384), (-0.181, -0.155)):
        prims["dark"].box((-0.145, 0.245, z0),
                          (-0.075, 1.310, z1))
    for y in (0.245, 0.765, 1.284):
        prims["dark"].box((-0.145, y, -0.410),
                          (-0.075, y + 0.026, -0.155))
    drive_sides = (32, 18, 10)[lod]
    # The lower control disc is substantially closed; the upper cable wheel
    # has the three open spokes drawn in Fig. 122.
    cylinder(prims["dark"], (-0.112, 0.525, -0.282),
             VR_DRIVE_STELL_RADIUS, 0.082,
             axis=(1, 0, 0), sides=drive_sides)
    spoked_wheel_yz(prims["dark"], (-0.112, 1.005, -0.282),
                    VR_DRIVE_SEILRAD_RADIUS, lod)
    for wheel_y in (0.525, 1.005):
        cylinder(prims["steel"], (-0.158, wheel_y, -0.282),
                 0.036, 0.018, axis=(1, 0, 0),
                 sides=(18, 12, 8)[lod])
    # This link owns the output joint consumed by ``vr_operating_rod_paths``.
    # It must survive every LOD; deleting it at LOD2 left the mast rod ending
    # at a mathematically correct but visibly empty point.
    beam(prims["dark"], (-0.165, 0.390, -0.315),
         (-0.165, 1.145, -0.245), 0.010, max(5, 9 - 2 * lod))
    cylinder(prims["dark"], (-0.165, 1.145, -0.245),
             0.021, 0.020, sides=max(8, 14 - 2 * lod))
    if lod < 2:
        # Slim cover straps, an operating crank and the vertical link between
        # both stages remain readable without turning the unit into a solid
        # metre-high block.
        prims["steel"].box((-0.151, 0.285, -0.423),
                           (-0.139, 1.270, -0.398))
        rect_beam(prims["dark"], (-0.168, 0.535, -0.315),
                  (-0.245, 0.300, -0.390), 0.025, 0.014)
        cylinder(prims["dark"], (-0.245, 0.300, -0.390),
                 0.025, 0.020, sides=(16, 10)[lod])


def vr_electric_drive_case(prims, centre_y, dimensions, lod,
                           output=False):
    """Closed Siemens cast drive case with a separately readable rear door."""
    width, height, depth = dimensions
    cx = VR_ELECTRIC_DRIVE_X
    # The drive is mounted behind the mast.  The rear-facing service door is
    # visible from the inspection side; from the signal front only its narrow
    # side shoulders may project beyond the 100-mm mast.
    front_z = -0.145
    rear_z = front_z - depth
    chamfer = 0.050 if lod == 0 else 0.042
    chamfered_box_xy(prims["steel"], (cx, centre_y), width, height,
                     rear_z, front_z, chamfer)

    # Recessed door seam and the slightly proud cast door.  Both preserved
    # J35 housings have large rounded shoulders, one central latch and a
    # bottom hinge/retaining strap rather than four decorative corner bolts.
    if lod < 2:
        chamfered_box_xy(prims["dark"], (cx, centre_y),
                         width - 0.034, height - 0.034,
                         rear_z - 0.009, rear_z - 0.001,
                         max(0.030, chamfer - 0.014))
        chamfered_box_xy(prims["steel"], (cx, centre_y),
                         width - 0.060, height - 0.060,
                         rear_z - 0.018, rear_z - 0.010,
                         max(0.024, chamfer - 0.020))
        # Central rectangular latch with a short rolled hinge barrel.
        latch_y = centre_y + 0.085 * height
        prims["dark"].box((cx - 0.038, latch_y - 0.046, rear_z - 0.030),
                          (cx + 0.038, latch_y + 0.046, rear_z - 0.019))
        prims["steel"].box((cx - 0.027, latch_y - 0.030, rear_z - 0.036),
                           (cx + 0.027, latch_y + 0.030, rear_z - 0.029))
        hinge_y = centre_y - height * 0.405
        prims["dark"].box((cx - 0.100, hinge_y - 0.026, rear_z - 0.027),
                          (cx + 0.100, hinge_y + 0.026, rear_z - 0.017))
        cylinder(prims["steel"], (cx, hinge_y, rear_z - 0.034),
                 0.022, 0.170, axis=(1, 0, 0),
                 sides=(18, 12)[lod])
        # The broad retaining strap passes over the rounded top of the case.
        strap_y = centre_y + height * 0.485
        prims["dark"].box((cx - 0.034, strap_y - 0.020, rear_z - 0.028),
                          (cx + 0.034, strap_y + 0.020, front_z + 0.010))
    if lod == 0:
        # One cast top plug and two small hinge screws are present in the
        # close prototype views.  Their neutral steel finish prevents a return
        # of the conspicuous orange pseudo-fasteners from older assets.
        cylinder(prims["steel"],
                 (cx, centre_y + height * 0.485, front_z - depth * 0.42),
                 0.026, 0.025, axis=(0, 1, 0), sides=16)
        hinge_y = centre_y - height * 0.405
        for x in (cx - 0.070, cx + 0.070):
            cylinder(prims["dark"], (x, hinge_y, rear_z - 0.039),
                     0.007, 0.006, sides=8)

    if output:
        # Side output shaft and forged coupling links seen beside the upper
        # Auskuppelaufsatz.  They connect to the two vertical rods without
        # implying a second, fictitious enclosure.
        side_x = cx + width * 0.5 + 0.020
        shaft_y = centre_y + 0.075
        shaft_z = front_z - depth * 0.52
        cylinder(prims["dark"], (side_x, shaft_y, shaft_z),
                 0.047, 0.055, axis=(1, 0, 0),
                 sides=(24, 16, 10)[lod])
        # The crank end is the real lower endpoint of both mast rods.  Keep a
        # simplified but continuous version in LOD2 instead of leaving those
        # rods suspended beside the drive case at the switching distance.
        rect_beam(prims["dark"],
                  (side_x + 0.020, shaft_y, shaft_z),
                  (side_x + 0.105, shaft_y + 0.170, shaft_z),
                  0.038, 0.016)
        cylinder(prims["dark"],
                 (side_x + 0.105, shaft_y + 0.170, shaft_z),
                 0.026, 0.020, sides=max(8, 16 - 4 * lod))


def vr_electric_drive(prims, aspects, lod):
    """Siemens motor drive; three-aspect signals add an Auskuppelaufsatz."""
    vr_electric_drive_case(
        prims, VR_ELECTRIC_DRIVE_LOWER_Y, VR_ELECTRIC_DRIVE_LOWER, lod)
    if aspects == 3:
        vr_electric_drive_case(
            prims, VR_ELECTRIC_DRIVE_UPPER_Y, VR_ELECTRIC_DRIVE_UPPER,
            lod, output=True)
    else:
        # The two-aspect drive still has one output crank on its main case.
        width, _height, depth = VR_ELECTRIC_DRIVE_LOWER
        side_x = VR_ELECTRIC_DRIVE_X + width * 0.5 + 0.020
        shaft_y = VR_ELECTRIC_DRIVE_LOWER_Y + 0.145
        shaft_z = -0.145 - depth * 0.52
        cylinder(prims["dark"], (side_x, shaft_y, shaft_z),
                 0.044, 0.050, axis=(1, 0, 0),
                 sides=(24, 16, 10)[lod])


def distant_drive_prims(drive, aspects, lod):
    """Return only the interchangeable foot-drive module for one Vr signal."""
    prims = base_prims()
    if drive == "drahtzug":
        vr_wire_drive(prims, lod)
    elif drive == "elektro":
        vr_electric_drive(prims, aspects, lod)
    else:
        raise ValueError(f"unknown Vr drive: {drive}")
    return prims


def vr_drive_output_joint(drive, aspects):
    """Return the existing physical joint where the mast rods meet a drive."""
    if aspects not in (2, 3):
        raise ValueError(f"unknown Vr aspect count: {aspects}")
    if drive == "drahtzug":
        # Upper end of the exposed link in ``vr_wire_drive``.
        return (-0.165, 1.145, -0.245)
    if drive != "elektro":
        raise ValueError(f"unknown Vr drive: {drive}")
    if aspects == 3:
        width, _height, depth = VR_ELECTRIC_DRIVE_UPPER
        side_x = VR_ELECTRIC_DRIVE_X + width * 0.5 + 0.020
        shaft_y = VR_ELECTRIC_DRIVE_UPPER_Y + 0.075
        shaft_z = -0.145 - depth * 0.52
        # End pin of the forged crank emitted by ``vr_electric_drive_case``.
        return (side_x + 0.105, shaft_y + 0.170, shaft_z)
    width, _height, depth = VR_ELECTRIC_DRIVE_LOWER
    side_x = VR_ELECTRIC_DRIVE_X + width * 0.5 + 0.020
    shaft_y = VR_ELECTRIC_DRIVE_LOWER_Y + 0.145
    shaft_z = -0.145 - depth * 0.52
    return (side_x, shaft_y, shaft_z)


def vr_upper_operating_joints(centre_y, aspects):
    """Return the fixed upper linkage pins for disc and optional wing rods."""
    joints = [(0.395, centre_y - 0.625, 0.177)]
    if aspects == 3:
        wing_axis_y = centre_y - 0.610 - VR_WING_PIVOT_FROM_TOP
        joints.append((-0.285, wing_axis_y + 0.385, 0.185))
    elif aspects != 2:
        raise ValueError(f"unknown Vr aspect count: {aspects}")
    return tuple(joints)


def vr_operating_rod_paths(centre_y, aspects, drive, mast_style):
    """Return positively connected disc/wing rod polylines.

    The first point is an actual joint already present on the selected drive;
    the last point is an actual upper bell-crank pin.  A two-aspect Vorsignal
    has only the disc rod, while its three-aspect counterpart adds exactly one
    independently guided rod for the Zusatzfluegel.
    """
    output = vr_drive_output_joint(drive, aspects)
    mast_width, mast_depth = vr_mast_envelope(mast_style)
    rod_x = mast_width * 0.5 + VR_OPERATING_ROD_MAST_CLEARANCE
    rod_z = -mast_depth * 0.5 - VR_OPERATING_ROD_REAR_OFFSET
    rod_xs = [rod_x]
    if aspects == 3:
        rod_xs.append(-rod_x)
    upper_joints = vr_upper_operating_joints(centre_y, aspects)
    paths = []
    for index, (rod_x, upper_joint) in enumerate(zip(rod_xs, upper_joints)):
        rear_y = output[1] + 0.030 + index * 0.020
        rod_bottom_y = output[1] + 0.080 + index * 0.020
        if rod_bottom_y >= upper_joint[1] - 0.040:
            raise ValueError(
                f"Vr {centre_y:g} m {aspects}-aspect {drive} rod has no "
                "positive connected span"
            )
        paths.append((
            output,
            (rod_x, rear_y, output[2]),
            (rod_x, rod_bottom_y, rod_z),
            (rod_x, upper_joint[1], rod_z),
            upper_joint,
        ))
    return tuple(paths)


def add_vr_operating_rods(prims, centre_y, aspects, drive, mast_style, lod):
    """Build the connected rods, their guides and the Zusatzfluegel crank."""
    paths = vr_operating_rod_paths(centre_y, aspects, drive, mast_style)
    mast_width, mast_depth = vr_mast_envelope(mast_style)
    mast_rear_z = -mast_depth * 0.5
    sides = max(6, 12 - 2 * lod)
    for path in paths:
        # Depth-changing and crosshead links are flat forgings.  The long
        # guided member itself is the round, painted Stellstange visible in
        # the Zeitz and Bad Waldsee detail photographs.
        rect_beam(prims["dark"], path[0], path[1], 0.026, 0.014)
        rect_beam(prims["dark"], path[1], path[2], 0.026, 0.014)
        beam(prims["steel"], path[2], path[3],
             VR_OPERATING_ROD_RADIUS, sides)
        rect_beam(prims["dark"], path[3], path[4], 0.030, 0.014)
        for point in (path[0], path[2], path[3], path[4]):
            cylinder(prims["dark"], point, 0.021, 0.020,
                     sides=max(8, 14 - 2 * lod))

        rod_bottom_y, rod_top_y = path[2][1], path[3][1]
        span = rod_top_y - rod_bottom_y
        guide_fractions = (0.50,) if span < 0.75 else (0.33, 0.70)
        side = 1.0 if path[2][0] > 0.0 else -1.0
        for fraction in guide_fractions:
            guide_y = rod_bottom_y + span * fraction
            rect_beam(
                prims["steel"],
                (side * (mast_width * 0.5), guide_y, mast_rear_z),
                (path[2][0], guide_y, path[2][2]),
                0.020,
                0.010,
            )
            cylinder(prims["dark"],
                     (path[2][0], guide_y, path[2][2]),
                     VR_OPERATING_ROD_RADIUS + 0.004, 0.030,
                     axis=(0, 1, 0), sides=max(8, 14 - 2 * lod))

    if aspects == 3:
        # A short, two-piece bell crank closes the second rod onto the fixed
        # Zusatzfluegel bearing.  Its final pin lies on the wing axis, so no
        # endpoint can detach merely because the wing changes aspect.
        wing_joint = vr_upper_operating_joints(centre_y, aspects)[1]
        wing_axis_y = centre_y - 0.610 - VR_WING_PIVOT_FROM_TOP
        intermediate = (-0.105, wing_axis_y + 0.205, 0.205)
        wing_axis = (0.000, wing_axis_y, 0.205)
        rect_beam(prims["dark"], wing_joint, intermediate, 0.038, 0.016)
        rect_beam(prims["dark"], intermediate, wing_axis, 0.038, 0.016)
        for point in (intermediate, wing_axis):
            cylinder(prims["dark"], point, 0.032, 0.024,
                     sides=max(8, 16 - 2 * lod))
        cylinder(prims["dark"], (0.0, wing_axis_y, 0.272), 0.038, 0.142,
                 sides=max(10, 20 - 4 * lod))


def distant_static_prims(centre_y, aspects, scheme, board_mode, lighting,
                         drive, mast_style, lod):
    prims = base_prims()
    foundation(prims, 0.44)
    # Stop the mast below the folding plane.  Extending the two channel rails
    # through it made them poke out of the orange face in Vr 1.
    # Freestanding unit-form distant signals use their dedicated narrow
    # Vorsignalmast.  The former broad lattice option was a main-signal mast
    # incorrectly inferred from the phrase "erhoehter Mast".
    if mast_style == "1944":
        vr_mast_1944(prims, centre_y - 0.055, scheme=scheme, lod=lod)
    elif mast_style == "alt_u":
        vr_mast_old_u(prims, centre_y - 0.055, scheme=scheme, lod=lod)
    else:
        raise ValueError(f"unknown Vr mast: {mast_style}")
    if board_mode == "am_mast":
        board_bottom = max(0.28, centre_y - 3.10)
        ne2_board(prims, board_bottom, "hoch", lod=lod)
    sides = (32, 20, 10)[lod]
    # The side reference shows a compact trunnion directly behind the disc: the
    # axle and its two cheek plates stay inside the one-metre silhouette instead
    # of extending as visible bars to either side.
    # Only the slim axle itself intersects the folding plane.  Its radius stays
    # below the enamel face, while the fixed cheeks sit wholly under the rear
    # sheet; no static bracket may pass through the raised disc.
    cylinder(prims["dark"], (0.0, centre_y, 0.275), 0.024, 0.34,
             axis=(1, 0, 0), sides=sides)
    for x in (-0.19, 0.13):
        prims["steel"].box((x, centre_y - 0.110, 0.035),
                           (x + 0.06, centre_y - 0.035, 0.255))
    beam(prims["steel"], (-0.16, centre_y - 0.06, 0.04),
         (-0.16, centre_y - 0.40, -0.08), 0.022, max(5, 8 - lod))
    beam(prims["steel"], (0.16, centre_y - 0.06, 0.04),
         (0.16, centre_y - 0.40, -0.08), 0.022, max(5, 8 - lod))
    # Do not duplicate the disc crank on the fixed mast.  The real crank is
    # bolted to the folding disc and is therefore already part of
    # ``vr_disc_mesh``.  The former fixed two-link chain stayed upright while
    # the disc folded to Vr 1 and became the conspicuous free black "cord"
    # above the lantern.  Only mast-supported rods and bearings belong here.
    add_vr_operating_rods(
        prims, centre_y, aspects, drive, mast_style, lod
    )
    add_vr_night_housings(
        prims, centre_y, lighting, mast_style, lod
    )
    return prims


def build_distant(centre_y, aspects, scheme="db_gruen",
                  lighting="led", board_mode="am_mast",
                  drive="drahtzug", mast_style="1944"):
    mesh_specs, nodes = [], []
    layout = vr_night_layout(centre_y)
    gas = lighting == "gas"
    for lod, _distance in VR_LODS:
        suffix = f"_LOD{lod}"
        mast_name = f"mast{suffix}"
        mesh_specs.append((mast_name, mapped(distant_static_prims(
            centre_y, aspects, scheme, board_mode, lighting, drive,
            mast_style, lod))))
        nodes.append({"name": mast_name, "mesh": mast_name})
        drive_name = f"antrieb{suffix}"
        mesh_specs.append((drive_name, mapped(distant_drive_prims(
            drive, aspects, lod))))
        nodes.append({"name": drive_name, "mesh": drive_name})
        disc_name = f"scheibe{suffix}"
        mesh_specs.append((disc_name, vr_disc_mesh(lod)))
        nodes.append({"name": disc_name, "mesh": disc_name,
                      "translation": [0, centre_y, 0.31]})
        if aspects == 3:
            wing_name = f"vorsignalfluegel{suffix}"
            mesh_specs.append((wing_name, vr_wing_mesh(lod)))
            nodes.append({"name": wing_name, "mesh": wing_name,
                          "translation": [
                              0,
                              centre_y - 0.61 - VR_WING_PIVOT_FROM_TOP,
                              0.36,
                          ]})
        for side, right in (("left", False), ("right", True)):
            selector_name = f"farbblende_{side}{suffix}"
            mesh_specs.append((selector_name,
                               vr_spectacle_mesh(right, lighting, lod)))
            x, y = layout["centres"][1 if right else 0]
            selector_z = 0.390 if gas else 0.335
            nodes.append({"name": selector_name, "mesh": selector_name,
                          "translation": [x, y, selector_z]})
        # Exactly one fixed light source in each lateral lantern.  Aspect
        # changes rotate the colour filters across these two axes; they never
        # switch between four separate electric lamps.
        fixed_axes = [layout["left_amber"], layout["right_amber"]]
        yellow_positions = fixed_axes
        green_positions = fixed_axes
        amber_light = "lit amber glass" if gas else MAT_LED_LIT_AMBER
        green_light = "lit green glass" if gas else MAT_LED_LIT_GREEN
        aspect_lights = {
            "vr_yellow": (yellow_positions, amber_light),
            "vr_green": (green_positions, green_light),
        }
        for base_name, (positions, material_name) in aspect_lights.items():
            name = f"{base_name}{suffix}"
            mesh_specs.append((name, vr_lamp_mesh(positions, material_name, lod, gas)))
            nodes.append({"name": name, "mesh": name})
        # DB Vr 2: exactly one yellow source at lower left and, rising to the
        # right, exactly one green source.  Separate nodes make it impossible
        # for a composite mesh/material edit to duplicate either colour.
        vr2_sources = {
            "vr2_yellow": (yellow_positions[0], amber_light),
            "vr2_green": (green_positions[1], green_light),
        }
        for base_name, (position, material_name) in vr2_sources.items():
            name = f"{base_name}{suffix}"
            mesh_specs.append((name, vr_lamp_mesh(
                [position], material_name, lod, gas)))
            nodes.append({"name": name, "mesh": name})
        if gas:
            bottle_name = f"gas_cartridges{suffix}"
            mesh_specs.append((bottle_name, gas_cartridge_mesh(centre_y, lod)))
            nodes.append({"name": bottle_name, "mesh": bottle_name})
    stem = str(centre_y).replace(".", "_")
    suffixes = []
    if scheme != "db_gruen":
        suffixes.append(scheme)
    if lighting != "led":
        suffixes.append(lighting)
    if drive != "drahtzug":
        suffixes.append("elektroantrieb")
    if mast_style != "1944":
        suffixes.append("altmast")
    if board_mode != "am_mast":
        suffixes.append("ohne_ne2")
    variant = "" if not suffixes else "_" + "_".join(suffixes)
    filename = f"sig_form_vr_{stem}m_{aspects}begr{variant}.gltf"
    write_gltf(filename, filename[:-5], materials_for(scheme), mesh_specs, nodes,
               external_textures=True)
    return filename


def build_ne2_standalone(size="hoch", scheme="db_gruen"):
    mesh_specs, nodes = [], []
    width, height = NE2_SIZES[size]
    board_bottom = 1.15 if size == "hoch" else 0.85
    for lod, _distance in VR_LODS:
        prims = base_prims()
        if lod < 2:
            foundation(prims, 0.38)
        post_half = 0.042 if lod < 2 else 0.055
        prims["steel"].box((-post_half, 0.17, -post_half),
                           (post_half, board_bottom, post_half))
        ne2_board(prims, board_bottom, size, lod=lod)
        name = f"tafel_LOD{lod}"
        mesh_specs.append((name, mapped(prims)))
        nodes.append({"name": name, "mesh": name})
    scheme_suffix = "" if scheme == "db_gruen" else f"_{scheme}"
    filename = f"sig_form_ne2_frei_{size}{scheme_suffix}.gltf"
    write_gltf(filename, filename[:-5], materials_for(scheme), mesh_specs, nodes,
               external_textures=True)
    return filename


def sh_post(prims, case_bottom, high, lod):
    """Plain paired-channel post used by high and dwarf form Sperrsignale.

    DB photographs and the signal-book elevation show a slim solid post.  The
    former reuse of the Hauptsignal lattice generator produced a fictional
    ladder mast.  Two rolled channels and sparse joining straps reproduce the
    front slot and the closed side elevation without inventing a lattice.
    """
    steel, dark, rust = prims["steel"], prims["dark"], prims["rust"]
    start = 0.145
    for x0, x1 in ((-0.073, -0.018), (0.018, 0.073)):
        steel.box((x0, start, -0.072), (x1, case_bottom, 0.072))
    strap_step = (0.92, 1.38, 2.40)[lod]
    y = start + 0.12
    while y < case_bottom - 0.08:
        steel.box((-0.080, y - 0.018, -0.080),
                  (0.080, y + 0.018, 0.080))
        y += strap_step
    # A pair of support cheeks carries the 700-mm housing.  High examples also
    # retain the two short maintenance stirrups and an exposed rear drive rod.
    steel.box((-0.115, case_bottom - 0.065, -0.105),
              (0.115, case_bottom + 0.030, 0.105))
    if high and lod < 2:
        stirrup_y = case_bottom - 0.27
        for side in (-1.0, 1.0):
            rect_beam(steel, (side * 0.055, stirrup_y, 0.0),
                      (side * 0.32, stirrup_y, 0.0), 0.027, 0.018)
            steel.box((side * 0.32 - 0.014, stirrup_y,
                       -0.010),
                      (side * 0.32 + 0.014, stirrup_y + 0.095,
                       0.010))
        beam(dark, (0.145, 0.42, -0.125),
             (0.145, case_bottom - 0.10, -0.125), 0.009,
             (8, 6)[lod])
    if lod == 0:
        for y in (0.30, min(case_bottom - 0.12, 2.18)):
            if y > start:
                rust.box((-0.081, y, -0.081),
                         (0.081, y + 0.006, 0.081))


def sh_static_prims(disc_y, high, scheme, lod):
    prims = base_prims()
    foundation(prims, 0.40 if high else 0.34)
    half_case = SH_CASE_SIZE * 0.5
    half_depth = SH_CASE_DEPTH * 0.5
    case_bottom = disc_y - half_case
    sh_post(prims, case_bottom, high, lod)

    # Exact 700 x 700 mm black housing; the 400-mm depth follows the surviving
    # 1938 drawing evaluation.  A 10-mm cap replaces the former oversized
    # cantilevered slab, which is absent from operational DB examples.
    prims["black"].box((-half_case, case_bottom, -half_depth),
                       (half_case, disc_y + half_case, half_depth))
    prims["dark"].box((-0.360, disc_y + half_case - 0.012, -0.210),
                      (0.360, disc_y + half_case + 0.012, 0.210))

    face_sides = (72, 36, 16)[lod]
    # The translucent/reflective white field is stationary.  Only the black
    # bar rotates, as the construction drawing and surviving signals show.
    cylinder(prims["white"], (0.0, disc_y, 0.211),
             SH_LIT_DISC_DIAMETER * 0.5, 0.022, sides=face_sides)
    annulus(prims["dark"], (0.0, disc_y), 0.305,
            SH_LIT_DISC_DIAMETER * 0.5, 0.201, 0.232,
            sides=face_sides)

    # Ril 301 rear day signs: two small white discs for Sh 0; the rotating
    # shutter covers the left one for Sh 1, leaving the right-hand disc.
    rear_radius = SH_REAR_MARKER_DIAMETER * 0.5
    for x in (-SH_REAR_MARKER_X, SH_REAR_MARKER_X):
        cylinder(prims["white"],
                 (x, disc_y + SH_REAR_MARKER_Y, -0.211),
                 rear_radius, 0.022, sides=(32, 20, 12)[lod])
        if lod < 2:
            annulus(prims["dark"],
                    (x, disc_y + SH_REAR_MARKER_Y),
                    rear_radius + 0.012, rear_radius,
                    -0.232, -0.201, sides=(32, 20)[lod])

    # Dark, flush fasteners around the face are visible in close photographs;
    # they are deliberately not rust-orange decorative dots.
    if lod == 0:
        fasteners = [(-0.315, -0.315), (0.315, -0.315),
                     (-0.315, 0.315), (0.315, 0.315),
                     (-0.315, 0.0), (0.315, 0.0)]
        for x, dy in fasteners:
            cylinder(prims["dark"], (x, disc_y + dy, 0.218),
                     0.010, 0.020, sides=10)

    # High installations carry the remote drive at the foot, not a large
    # invented box immediately below the signal face.
    if high:
        prims["steel"].box((-0.225, 0.18, -0.205),
                           (0.165, 0.78, 0.205))
        if lod < 2:
            prims["dark"].box((-0.208, 0.205, 0.207),
                              (0.148, 0.755, 0.220))
            for y in (0.28, 0.68):
                cylinder(prims["dark"], (-0.190, y, 0.228),
                         0.012, 0.020, sides=(10, 8)[lod])
    return prims


def sh_bar_mesh(lod):
    black, dark = Prim(), Prim()
    # Front bar: horizontal for Sh 0, +45 degrees (right end rising) for Sh 1.
    black.box((-0.260, -0.056, 0.225), (0.260, 0.056, 0.263))
    cylinder(dark, (0.0, 0.0, 0.268), 0.035, 0.018,
             sides=(28, 18, 10)[lod])

    # Rear occulting shutter.  Its rest point lies invisibly on the black
    # background; after the same +45-degree turn it covers the left marker.
    # A rear observer sees model +X on the left.  Cover that marker so the
    # right-hand rear marker remains visible in Sh 1, exactly as in SB-DB 1959.
    tx, ty = SH_REAR_MARKER_X, SH_REAR_MARKER_Y
    c = math.sqrt(0.5)
    shutter_x = c * tx + c * ty
    shutter_y = -c * tx + c * ty
    cylinder(black, (shutter_x, shutter_y, -0.232),
             SH_REAR_MARKER_DIAMETER * 0.62, 0.024,
             sides=(32, 20, 10)[lod])
    cylinder(dark, (0.0, 0.0, -0.229), 0.044, 0.028,
             sides=(28, 18, 10)[lod])
    if lod < 2:
        # Compact crank on the axle; the long pull rod remains fixed to the
        # mast and is not incorrectly rotated with the display bar.
        rect_beam(dark, (0.0, 0.0, -0.245),
                  (0.175, -0.185, -0.245), 0.026, 0.014)
        cylinder(dark, (0.175, -0.185, -0.245), 0.028, 0.022,
                 sides=(16, 10)[lod])
    return {MAT_BLACK: black, MAT_DARK: dark}


def sh_light_mesh(lod):
    """Internal opal illumination shared by both mechanical aspects."""
    lens = Prim()
    sides = (64, 32, 16)[lod]
    cylinder(lens, (0.0, 0.0, 0.224),
             SH_LIT_DISC_DIAMETER * 0.475, 0.006, sides=sides)
    for x in (-SH_REAR_MARKER_X, SH_REAR_MARKER_X):
        cylinder(lens, (x, SH_REAR_MARKER_Y, -0.225),
                 SH_REAR_MARKER_DIAMETER * 0.44, 0.006, sides=sides)
    return {"lit warm glass": lens}


def build_sh(high=False, scheme="db_gruen"):
    disc_y = SH_HIGH_CENTRE if high else SH_LOW_CENTRE
    mesh_specs, nodes = [], []
    for lod, _distance in SIGNAL_LODS:
        suffix = f"_LOD{lod}"
        static_name = f"traeger{suffix}"
        bar_name = f"sperrscheibe{suffix}"
        light_name = f"sh_white{suffix}"
        mesh_specs += [
            (static_name, mapped(sh_static_prims(disc_y, high, scheme, lod))),
            (bar_name, sh_bar_mesh(lod)),
            (light_name, sh_light_mesh(lod)),
        ]
        nodes += [
            {"name": static_name, "mesh": static_name},
            {"name": bar_name, "mesh": bar_name,
             "translation": [0, disc_y, 0]},
            {"name": light_name, "mesh": light_name,
             "translation": [0, disc_y, 0]},
        ]
    suffix = "" if scheme == "db_gruen" else f"_{scheme}"
    filename = f"sig_form_sh_{'hoch' if high else 'niedrig'}{suffix}.gltf"
    write_gltf(filename, filename[:-5], materials_for(scheme), mesh_specs, nodes,
               external_textures=True)
    return filename


def write_model(path, text):
    path.write_text("// Generated by tools/gen_form_signals.py — edit the generator.\n" + text,
                    encoding="utf-8", newline="\n")


def main_model_ron(asset, arms, tags, coupled=False):
    if coupled and arms != 2:
        raise ValueError("only a two-arm Hauptsignal can use the coupled drive")
    lamps = []
    motions = []
    # A restitution near 0.35 produces a clearly readable first hop of roughly
    # five degrees at the stop, followed by a much smaller second hop.  Bind
    # every LOD explicitly so switching range never freezes the mechanism.
    for level, _distance in HP_LODS:
        suffix = f"_LOD{level}"
        lamps += [
            f'        (lamp: "lamp_red", node: "lamp_red{suffix}"),',
            f'        (lamp: "lamp_green", node: "lamp_green{suffix}"),',
        ]
        if arms == 2:
            lamps.append(
                f'        (lamp: "lamp_yellow", node: "lamp_yellow{suffix}"),')
        upper_channel = "fluegel_gekuppelt" if coupled else "fluegel1"
        upper_fall = 0.80 if coupled else 0.75
        upper_rebound = 0.35 if coupled else 0.36
        motions += [
            f'        (lamp: "{upper_channel}", node: "fluegel1{suffix}", '
            'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: 45.0), '
            f'seconds: 1.8, profile: Semaphore(fall_seconds: {upper_fall:.2f}, '
            f'rebound: {upper_rebound:.2f})),',
            f'        (lamp: "{upper_channel}", node: "gewicht1{suffix}", '
            'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: 45.0), '
            f'seconds: 1.8, profile: Semaphore(fall_seconds: {upper_fall:.2f}, '
            f'rebound: {upper_rebound:.2f})),',
            f'        (lamp: "{upper_channel}", '
            f'node: "gewicht_ausgleich1{suffix}", '
            'motion: Rotate(axis: (0.0, 0.0, 1.0), '
            f'degrees: {-HP_EQUALIZER_SWING_DEGREES:.1f}), '
            f'seconds: 1.8, profile: Semaphore(fall_seconds: {upper_fall:.2f}, '
            f'rebound: {upper_rebound:.2f})),',
            f'        (lamp: "{upper_channel}", node: "blende1{suffix}", '
            f'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: '
            f'{HP_SELECTOR_SWING_DEGREES:.1f}), '
            f'seconds: 1.8, profile: Semaphore(fall_seconds: {upper_fall:.2f}, '
            f'rebound: {upper_rebound:.2f})),',
        ]
        if arms == 2:
            lower_channel = "fluegel_gekuppelt" if coupled else "fluegel2"
            lower_fall = 0.80 if coupled else 0.85
            lower_rebound = 0.35 if coupled else 0.34
            motions += [
                f'        (lamp: "{lower_channel}", node: "fluegel2{suffix}", '
                'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: -45.0), '
                f'seconds: 1.8, profile: Semaphore(fall_seconds: {lower_fall:.2f}, '
                f'rebound: {lower_rebound:.2f})),',
                f'        (lamp: "{lower_channel}", node: "gewicht2{suffix}", '
                'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: -45.0), '
                f'seconds: 1.8, profile: Semaphore(fall_seconds: {lower_fall:.2f}, '
                f'rebound: {lower_rebound:.2f})),',
                f'        (lamp: "{lower_channel}", '
                f'node: "gewicht_ausgleich2{suffix}", '
                'motion: Rotate(axis: (0.0, 0.0, 1.0), '
                f'degrees: {HP_EQUALIZER_SWING_DEGREES:.1f}), '
                f'seconds: 1.8, profile: Semaphore(fall_seconds: {lower_fall:.2f}, '
                f'rebound: {lower_rebound:.2f})),',
                f'        (lamp: "{lower_channel}", node: "blende2{suffix}", '
                f'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: '
                f'{-HP_SELECTOR_SWING_DEGREES:.1f}), '
                f'seconds: 1.8, profile: Semaphore(fall_seconds: {lower_fall:.2f}, '
                f'rebound: {lower_rebound:.2f})),',
            ]
    lods = "\n".join(
        f"        (level: {level}, distance: {distance:.1f}),"
        for level, distance in HP_LODS
    )
    return f'''(
    parts: [(file: "example/assets/{asset}")],
    lamps: [
{chr(10).join(lamps)}
    ],
    motions: [
{chr(10).join(motions)}
    ],
    lods: [
{lods}
    ],
    tags: [{', '.join(f'"{tag}"' for tag in tags)}],
)
'''


def distant_model_ron(asset, aspects, lighting, tags):
    lamps = []
    motions = []
    for level, _distance in VR_LODS:
        suffix = f"_LOD{level}"
        lamps += [
            f'        (lamp: "vr0_licht", node: "vr_yellow{suffix}"),',
            f'        (lamp: "vr1_licht", node: "vr_green{suffix}"),',
            f'        (lamp: "vr2_licht", node: "vr2_yellow{suffix}"),',
            f'        (lamp: "vr2_licht", node: "vr2_green{suffix}"),',
        ]
        motions.append(
            f'        (lamp: "scheibe_weg", node: "scheibe{suffix}", '
            'motion: Rotate(axis: (1.0, 0.0, 0.0), degrees: -90.0), '
            'seconds: 1.6, profile: Semaphore(fall_seconds: 0.90, rebound: 0.08)),'
        )
        if aspects == 3:
            motions.append(
                f'        (lamp: "vr2_fluegel", node: "vorsignalfluegel{suffix}", '
                'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: 45.0), '
                'seconds: 1.6, profile: Semaphore(fall_seconds: 0.72, rebound: 0.18)),'
            )
        # Both the gas and electric designs use one stationary light source per
        # side.  Their mechanically coupled two-colour filter carriers exchange
        # amber and green by half-turns.  For Vr 2 only the right carrier turns.
        motions += [
            f'        (lamp: "vr_blende_links_gruen", node: "farbblende_left{suffix}", '
            'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: 180.0), '
            'seconds: 1.15, profile: Linear),',
            f'        (lamp: "vr_blende_rechts_gruen", node: "farbblende_right{suffix}", '
            'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: -180.0), '
            'seconds: 1.15, profile: Linear),',
        ]
    lods = "\n".join(
        f"        (level: {level}, distance: {distance:.1f}),"
        for level, distance in VR_LODS
    )
    return f'''(
    parts: [(file: "example/assets/{asset}")],
    lamps: [
{chr(10).join(lamps)}
    ],
    motions: [
{chr(10).join(motions)}
    ],
    lods: [
{lods}
    ],
    tags: [{', '.join(f'"{tag}"' for tag in tags)}],
)
'''


def ne2_model_ron(asset, tags):
    lods = "\n".join(
        f"        (level: {level}, distance: {distance:.1f}),"
        for level, distance in VR_LODS
    )
    return f'''(
    parts: [(file: "example/assets/{asset}")],
    lods: [
{lods}
    ],
    tags: [{', '.join(f'"{tag}"' for tag in tags)}],
)
'''


def sh_model_ron(asset, tags):
    lamps = []
    motions = []
    for level, _distance in SIGNAL_LODS:
        suffix = f"_LOD{level}"
        lamps.append(
            f'        (lamp: "sh_white", node: "sh_white{suffix}"),')
        motions.append(
            f'        (lamp: "sperrscheibe_frei", node: "sperrscheibe{suffix}", '
            'motion: Rotate(axis: (0.0, 0.0, 1.0), degrees: 45.0), '
            'seconds: 1.2, profile: Linear),'
        )
    lods = "\n".join(
        f"        (level: {level}, distance: {distance:.1f}),"
        for level, distance in SIGNAL_LODS
    )
    return f'''(
    parts: [(file: "example/assets/{asset}")],
    lamps: [
{chr(10).join(lamps)}
    ],
    motions: [
{chr(10).join(motions)}
    ],
    lods: [
{lods}
    ],
    tags: [{', '.join(f'"{tag}"' for tag in tags)}],
)
'''


def validate_asset(asset, expected_height=None, required_nodes=(), expected_scheme=None,
                   expected_ne2_size=None, lod_stems=(), expect_gas=False,
                   expected_vr_centre=None):
    import base64
    import json
    import struct

    asset_path = ROOT / "mods" / "example" / "assets" / asset
    gltf = json.loads(asset_path.read_text())

    def image_bytes(image):
        uri = image["uri"]
        if uri.startswith("data:"):
            return base64.b64decode(uri.split(",", 1)[1])
        path = (asset_path.parent / uri).resolve()
        assert path.is_relative_to(asset_path.parent.resolve()), \
            f"{asset}: texture escapes the asset root"
        assert path.is_file(), f"{asset}: missing external texture {uri}"
        return path.read_bytes()

    assert len(gltf.get("images", [])) >= 3, f"{asset}: PBR maps missing"
    assert all(image["uri"].startswith("textures/signals/")
               for image in gltf["images"]), \
        f"{asset}: form-signal PBR maps must be shared external assets"
    assert all("metallicRoughnessTexture" in mat["pbrMetallicRoughness"]
               for mat in gltf["materials"]), f"{asset}: material lacks ORM map"
    assert all("normalTexture" in mat and "occlusionTexture" in mat
               for mat in gltf["materials"]), f"{asset}: normal/AO map missing"
    assert all(all(0.0 <= value <= 1.0 for value in mat.get("emissiveFactor", ()))
               for mat in gltf["materials"]), f"{asset}: invalid emissive factor"
    for lens in (mat for mat in gltf["materials"]
                 if mat["name"].endswith("filter glass")):
        extensions = lens.get("extensions", {})
        transmission = extensions["KHR_materials_transmission"]["transmissionFactor"]
        ior = extensions["KHR_materials_ior"]["ior"]
        thickness = extensions["KHR_materials_volume"]["thicknessFactor"]
        if lens["name"].startswith("LED "):
            assert transmission >= 0.80
            assert 1.48 <= ior <= 1.50
            assert 0.0 < thickness <= 0.003
            assert lens["pbrMetallicRoughness"]["roughnessFactor"] <= 0.04
            assert lens["normalTexture"]["scale"] <= 0.004
        else:
            assert transmission >= 0.4
            assert 1.50 <= ior <= 1.54
            assert thickness > 0.0
            assert lens["pbrMetallicRoughness"]["roughnessFactor"] <= 0.08
            assert lens["normalTexture"]["scale"] <= 0.01
    node_map = {node["name"]: node for node in gltf["nodes"]}
    nodes = set(node_map)
    assert nodes, f"{asset}: no nodes"
    assert set(required_nodes) <= nodes, f"{asset}: missing nodes {set(required_nodes) - nodes}"
    used_materials = {
        gltf["materials"][primitive["material"]]["name"]
        for mesh in gltf["meshes"] for primitive in mesh["primitives"]
    }
    if "vr_yellow_LOD0" in nodes:
        assert expected_vr_centre in VR_CENTRE_HEIGHTS
        assert "_gitter" not in asset, f"{asset}: unsupported Vr lattice mast returned"
        for level, _distance in VR_LODS:
            disc_node = node_map[f"scheibe_LOD{level}"]
            assert abs(disc_node["translation"][1] - expected_vr_centre) < 1e-6
            disc_mesh = gltf["meshes"][disc_node["mesh"]]
            positions = [gltf["accessors"][primitive["attributes"]["POSITION"]]
                         for primitive in disc_mesh["primitives"]]
            min_x = min(accessor["min"][0] for accessor in positions)
            max_x = max(accessor["max"][0] for accessor in positions)
            min_y = min(accessor["min"][1] for accessor in positions)
            max_y = max(accessor["max"][1] for accessor in positions)
            assert abs(min_x + 0.5) < 1e-6 and abs(max_x - 0.5) < 1e-6
            assert abs(min_y + 0.5) < 1e-6 and abs(max_y - 0.5) < 1e-6
            # Yellow and green are alternative filters over the same physical
            # lamp axes, never four independently positioned electric lamps.
            yellow_node = node_map[f"vr_yellow_LOD{level}"]
            green_node = node_map[f"vr_green_LOD{level}"]
            yellow_mesh = gltf["meshes"][yellow_node["mesh"]]
            green_mesh = gltf["meshes"][green_node["mesh"]]
            yellow_bounds = [gltf["accessors"][p["attributes"]["POSITION"]]
                             for p in yellow_mesh["primitives"]]
            green_bounds = [gltf["accessors"][p["attributes"]["POSITION"]]
                            for p in green_mesh["primitives"]]
            assert [(a["min"], a["max"]) for a in yellow_bounds] == [
                (a["min"], a["max"]) for a in green_bounds
            ], f"{asset}: electric/gas colours do not share the two fixed lamp axes"
            wing_name = f"vorsignalfluegel_LOD{level}"
            if wing_name in node_map:
                wing_mesh = gltf["meshes"][node_map[wing_name]["mesh"]]
                wing_bounds = [
                    gltf["accessors"][primitive["attributes"]["POSITION"]]
                    for primitive in wing_mesh["primitives"]
                ]
                wing_width = (
                    max(bounds["max"][0] for bounds in wing_bounds)
                    - min(bounds["min"][0] for bounds in wing_bounds)
                )
                wing_length = (
                    max(bounds["max"][1] for bounds in wing_bounds)
                    - min(bounds["min"][1] for bounds in wing_bounds)
                )
                assert abs(wing_width - VR_WING_WIDTH) < 1e-6
                assert abs(wing_length - VR_WING_LENGTH) < 1e-6
        gas_filters = {"green filter glass", "amber filter glass"}
        led_filters = {MAT_LED_GREEN_FILTER, MAT_LED_AMBER_FILTER}
        expected_filters, forbidden_filters = ((gas_filters, led_filters)
                                                if expect_gas
                                                else (led_filters, gas_filters))
        assert expected_filters <= used_materials, f"{asset}: wrong filter-glass family"
        assert not forbidden_filters & used_materials, f"{asset}: mixed gas/LED filter glass"
        # Mesh depth is independent of the volume shader thickness.  Enforce
        # the visibly flatter electric cover so a material-only edit cannot
        # silently restore the old deep gas-lens silhouette.
        relevant_depths = []
        for mesh in gltf["meshes"]:
            for primitive in mesh["primitives"]:
                material_name = gltf["materials"][primitive["material"]]["name"]
                if material_name in expected_filters:
                    bounds = gltf["accessors"][primitive["attributes"]["POSITION"]]
                    relevant_depths.append(bounds["max"][2] - bounds["min"][2])
        assert relevant_depths
        if expect_gas:
            assert min(relevant_depths) >= 0.020
        else:
            assert max(relevant_depths) <= 0.0083
    # Vr 2 (DS 301) is one yellow light at lower left plus one green light
    # rising to the right.  Validate colour, count and placement independently
    # for every LOD, rather than trusting a shared composite lamp mesh.
    if "vr2_yellow_LOD0" in nodes:
        for level, _distance in VR_LODS:
            yellow_name = f"vr2_yellow_LOD{level}"
            green_name = f"vr2_green_LOD{level}"
            yellow_mesh = gltf["meshes"][node_map[yellow_name]["mesh"]]
            green_mesh = gltf["meshes"][node_map[green_name]["mesh"]]
            assert len(yellow_mesh["primitives"]) == 1
            assert len(green_mesh["primitives"]) == 1
            yellow_prim = yellow_mesh["primitives"][0]
            green_prim = green_mesh["primitives"][0]
            expected_amber = "lit amber glass" if expect_gas else MAT_LED_LIT_AMBER
            expected_green = "lit green glass" if expect_gas else MAT_LED_LIT_GREEN
            assert gltf["materials"][yellow_prim["material"]]["name"] == expected_amber
            assert gltf["materials"][green_prim["material"]]["name"] == expected_green
            yellow_box = gltf["accessors"][yellow_prim["attributes"]["POSITION"]]
            green_box = gltf["accessors"][green_prim["attributes"]["POSITION"]]
            assert yellow_box["max"][0] < 0.0 < green_box["min"][0]
            assert yellow_box["max"][1] < green_box["min"][1]
    if "sperrscheibe_LOD0" in nodes:
        expected_centre = SH_HIGH_CENTRE if "_hoch" in asset else SH_LOW_CENTRE
        mesh_by_name = {mesh["name"]: mesh for mesh in gltf["meshes"]}
        material_names = {index: material["name"]
                          for index, material in enumerate(gltf["materials"])}
        for level, _distance in SIGNAL_LODS:
            for stem in ("sperrscheibe", "sh_white"):
                node = node_map[f"{stem}_LOD{level}"]
                assert abs(node["translation"][1] - expected_centre) < 1e-6
            moving = mesh_by_name[f"sperrscheibe_LOD{level}"]
            moving_materials = {
                material_names[primitive["material"]]
                for primitive in moving["primitives"]
            }
            assert MAT_WHITE not in moving_materials, \
                f"{asset}: the fixed Sh white disc moves with its black bar"
            static = mesh_by_name[f"traeger_LOD{level}"]
            white_bounds = [
                gltf["accessors"][primitive["attributes"]["POSITION"]]
                for primitive in static["primitives"]
                if material_names[primitive["material"]] == MAT_WHITE
            ]
            assert white_bounds, f"{asset}: stationary Sh white disc missing"
            min_x = min(bounds["min"][0] for bounds in white_bounds)
            max_x = max(bounds["max"][0] for bounds in white_bounds)
            min_y = min(bounds["min"][1] for bounds in white_bounds)
            max_y = max(bounds["max"][1] for bounds in white_bounds)
            assert abs((max_x - min_x) - SH_LIT_DISC_DIAMETER) < 1e-6
            assert abs((max_y - min_y) - SH_LIT_DISC_DIAMETER) < 1e-6
    if expected_height is not None:
        arm = next(node for node in gltf["nodes"]
                   if node["name"] == "fluegel1_LOD0")
        assert abs(arm["translation"][1] - expected_height) < 1e-6
    materials = {mat["name"]: mat["pbrMetallicRoughness"]
                 for mat in gltf["materials"]}
    if MAT_DARK in materials:
        dark = materials[MAT_DARK]
        assert dark["metallicFactor"] <= 0.03 and dark["roughnessFactor"] >= 0.82, \
            f"{asset}: rear faces/ironwork are not matte black paint"
    if "laterne1_LOD0" in nodes:
        galvanised = materials[MAT_GALVANISED]
        assert 0.20 <= galvanised["metallicFactor"] <= 0.40
        assert 0.62 <= galvanised["roughnessFactor"] <= 0.75
    # Fired signal faces must remain smooth.  This guard catches the former
    # coarse normal profile and overly matte factors that made intact enamel
    # resemble paper or felt in close game-renderer previews.
    enamel_names = {MAT_RED, MAT_WHITE, MAT_YELLOW, MAT_VR_ORANGE}
    for enamel in (mat for mat in gltf["materials"]
                   if mat["name"] in enamel_names):
        pbr = enamel["pbrMetallicRoughness"]
        assert pbr["metallicFactor"] <= 0.04
        assert 0.45 <= pbr["roughnessFactor"] <= 0.55
        assert enamel["normalTexture"]["scale"] <= 0.06
        clearcoat = enamel.get("extensions", {}).get("KHR_materials_clearcoat")
        assert clearcoat is not None, f"{asset}: enamel clearcoat missing"
        assert 0.20 <= clearcoat["clearcoatFactor"] <= 0.35
        assert 0.30 <= clearcoat["clearcoatRoughnessFactor"] <= 0.45
        assert clearcoat["clearcoatNormalTexture"]["scale"] <= 0.03
    if expected_scheme is not None:
        actual = materials[MAT_STRUCTURE]["baseColorFactor"][:3]
        expected = PAINT_SCHEMES[expected_scheme]["structure"]
        assert all(abs(a - e) < 1e-6 for a, e in zip(actual, expected)), \
            f"{asset}: wrong {expected_scheme} structure colour"
    if lod_stems:
        for level, _distance in VR_LODS:
            assert any(name.endswith(f"_LOD{level}") for name in nodes), \
                f"{asset}: LOD{level} node missing"
        # LOD meshes must actually become cheaper, not merely carry new names.
        mesh_by_name = {mesh["name"]: mesh for mesh in gltf["meshes"]}
        for stem in lod_stems:
            counts = [sum(gltf["accessors"][prim["indices"]]["count"]
                          for prim in mesh_by_name[f"{stem}_LOD{level}"]["primitives"])
                      for level, _distance in VR_LODS]
            assert counts[0] > counts[1] > counts[2], \
                f"{asset}: {stem} LODs do not reduce triangles: {counts}"
    gas_nodes = {name for name in nodes if name.startswith("gas_cartridges_LOD")}
    assert bool(gas_nodes) == expect_gas, f"{asset}: gas cartridge variant mismatch"

    ne2_indices = [i for i, mat in enumerate(gltf["materials"])
                   if mat["name"] == MAT_NE2]
    ne2_primitives = ([prim for mesh in gltf["meshes"] for prim in mesh["primitives"]
                       if prim["material"] == ne2_indices[0]]
                      if ne2_indices else [])
    if expected_ne2_size is None:
        assert not ne2_primitives, f"{asset}: unexpected Ne 2 face"
    else:
        assert ne2_indices, f"{asset}: Ne 2 material missing"
        ne2_index = ne2_indices[0]
        width, height = NE2_SIZES[expected_ne2_size]
        for prim in ne2_primitives:
            accessor = gltf["accessors"][prim["attributes"]["POSITION"]]
            actual_width = accessor["max"][0] - accessor["min"][0]
            actual_height = accessor["max"][1] - accessor["min"][1]
            assert abs(actual_width - width) < 1e-6
            assert abs(actual_height - height) < 1e-6
        pbr = gltf["materials"][ne2_index]["pbrMetallicRoughness"]
        texture = gltf["textures"][pbr["baseColorTexture"]["index"]]
        image = gltf["images"][texture["source"]]
        png = image_bytes(image)
        assert struct.unpack(">II", png[16:24]) == (1024, 1024), \
            f"{asset}: Ne 2 face map is not 1024 px"
    # Accessor bounds are the contract that all geometry remains full-size.
    data = base64.b64decode(gltf["buffers"][0]["uri"].split(",", 1)[1])
    assert len(data) == gltf["buffers"][0]["byteLength"]
    for image in gltf["images"]:
        png = image_bytes(image)
        assert png.startswith(b"\x89PNG\r\n\x1a\n") and png.endswith(b"IEND\xaeB`\x82")
        width, height = struct.unpack(">II", png[16:24])
        assert width >= 512 and height >= 512, f"{asset}: PBR map below 512 px"
    assert gltf["samplers"][0]["magFilter"] == 9729
    assert gltf["samplers"][0]["minFilter"] == 9987
    for accessor in gltf["accessors"]:
        if accessor["componentType"] == 5126:
            view = gltf["bufferViews"][accessor["bufferView"]]
            assert view["byteLength"] % struct.calcsize("f") == 0


def validate_dimensions():
    """Keep documented dimensions and reconstructions stable across refactors."""
    assert HP_PIVOT_HEIGHTS == (6.00, 8.00, 10.00, 12.00, 14.00)
    assert HP_RAIL_HEIGHT == 0.172
    assert abs(hp_pivot_above_so(8.0) - 7.828) < 1e-12
    assert hp_pivot_levels(8.0) == (7.828, 5.628)
    assert abs(hp_pivot_levels(8.0)[0] - hp_pivot_levels(8.0)[1]
               - HP_ARM_PIVOT_SPACING) < 1e-12
    assert HP_ARM[(False, False)] == (2.220, 0.240, 0.450)
    assert HP_ARM[(False, True)] == (1.820, 0.240, 0.450)
    assert HP_ARM[(True, False)] == (1.920, 0.220, 0.420)
    assert HP_ARM[(True, True)] == (1.520, 0.220, 0.420)
    assert HP_ARM_PIVOT_SPACING == 2.200
    assert VR_DISC_DIAMETER == 1.000
    assert VR_DISC_WHITE_RADIUS == 0.490
    assert VR_DISC_BLACK_RING_RADIUS == 0.435
    assert VR_DISC_FACE_RADIUS == 0.400
    assert VR_DISC_DIAMETER * 0.5 > VR_DISC_WHITE_RADIUS \
        > VR_DISC_BLACK_RING_RADIUS > VR_DISC_FACE_RADIUS
    assert VR_WING_LENGTH == 1.410
    assert VR_WING_WIDTH == 0.240
    assert VR_WING_PIVOT_FROM_TOP == 0.705
    assert VR_WING_EDGE_FASTENERS_FROM_TOP \
        == (0.120, 0.365, 0.610, 0.855, 1.100)
    assert all(0.0 < distance < 1.166
               for distance in VR_WING_EDGE_FASTENERS_FROM_TOP)
    assert VR_CENTRE_HEIGHTS == (2.76, 4.87, 5.37)
    assert VR_DRIVE_SEILRAD_RADIUS > VR_DRIVE_STELL_RADIUS
    drive_lod_counts = []
    for lod in range(3):
        wheel = Prim()
        spoked_wheel_yz(wheel, (0.0, 0.0, 0.0),
                        VR_DRIVE_SEILRAD_RADIUS, lod)
        wheel_y = [y for _x, y, _z in wheel.pos]
        wheel_z = [z for _x, _y, z in wheel.pos]
        for extent in (max(wheel_y) - min(wheel_y),
                       max(wheel_z) - min(wheel_z)):
            # Coarse LOD polygons need not place a vertex at every cardinal
            # point, but their envelope must preserve at least 95% of the
            # intended diameter and may never grow beyond it.
            assert 1.89 * VR_DRIVE_SEILRAD_RADIUS <= extent \
                <= 2.001 * VR_DRIVE_SEILRAD_RADIUS
        if lod == 0:
            assert abs(max(wheel_y) - min(wheel_y)
                       - 2.0 * VR_DRIVE_SEILRAD_RADIUS) < 1e-9
            assert abs(max(wheel_z) - min(wheel_z)
                       - 2.0 * VR_DRIVE_SEILRAD_RADIUS) < 1e-9
        drive_lod_counts.append(len(wheel.idx))
    assert drive_lod_counts[0] > drive_lod_counts[1] > drive_lod_counts[2]
    assert VR_ELECTRIC_DRIVE_LOWER == (0.410, 0.760, 0.340)
    assert VR_ELECTRIC_DRIVE_UPPER == (0.380, 0.570, 0.310)
    # Each drive is a separately generated module with genuinely cheaper
    # LODs.  The three-aspect electrical version must contain more geometry
    # and a taller envelope than the two-aspect version because only it has
    # the documented Auskuppelaufsatz.
    drive_counts = {}
    drive_heights = {}
    for drive in ("drahtzug", "elektro"):
        for aspects in (2, 3):
            counts = []
            for lod in range(3):
                module = distant_drive_prims(drive, aspects, lod)
                counts.append(sum(len(primitive.idx)
                                  for primitive in module.values()))
                if lod == 0:
                    points = [point for primitive in module.values()
                              for point in primitive.pos]
                    drive_heights[(drive, aspects)] = (
                        max(y for _x, y, _z in points)
                        - min(y for _x, y, _z in points))
            assert counts[0] > counts[1] > counts[2]
            drive_counts[(drive, aspects)] = counts[0]
    assert drive_counts[("drahtzug", 2)] == drive_counts[("drahtzug", 3)]
    assert drive_counts[("elektro", 3)] > drive_counts[("elektro", 2)]
    assert drive_heights[("elektro", 3)] \
        > drive_heights[("elektro", 2)] + 0.50
    assert NE2_SIZES == {"hoch": (0.480, 0.750), "niedrig": (0.300, 0.450)}
    assert LANTERN_GLASS_DIAMETER == 0.170
    assert VR_LED_LANTERN_CASE == (0.220, 0.490)
    assert VR_LANTERN_APERTURE_SPACING == 0.270
    assert VR_LANTERN_LATERAL_OFFSET == 0.275
    assert VR_LANTERN_RIGHT_DROP == 0.805
    assert VR_LANTERN_LEFT_DROP == 1.315
    assert abs(VR_LANTERN_LEFT_DROP - VR_LANTERN_RIGHT_DROP - 0.510) < 1e-12
    assert VR_LED_LENS_CENTRE_DEPTH == 0.0082
    assert VR_LED_LENS_CENTRE_DEPTH < 0.4 * 0.023
    assert VR_LED_FRESNEL_STEP < 0.25 * 0.0007
    wing = vr_wing_mesh(0)
    wing_x = [x for primitive in wing.values()
              for x, _y, _z in primitive.pos]
    wing_y = [y for primitive in wing.values()
              for _x, y, _z in primitive.pos]
    assert abs((max(wing_x) - min(wing_x)) - VR_WING_WIDTH) < 1e-9
    assert abs((max(wing_y) - min(wing_y)) - VR_WING_LENGTH) < 1e-9
    assert abs(max(wing_y) - VR_WING_PIVOT_FROM_TOP) < 1e-9
    assert abs(min(wing_y) + VR_WING_PIVOT_FROM_TOP) < 1e-9
    assert min(z for _x, _y, z in wing[MAT_WHITE].pos) > 0.0
    assert min(z for _x, _y, z in wing[MAT_BLACK].pos) <= 0.0
    assert HP_RED_BOARD_LENGTH == 0.998
    assert HP_WHITE_BOARD_LENGTH == 0.998
    assert HP_ARM_ROOT == {False: 0.400, True: 0.400}
    assert HP_ARM_WHITE_DISC_RADIUS == {False: 0.135, True: 0.128}
    for height, fields in zip(HP_PIVOT_HEIGHTS, (3, 5, 7, 9, 11)):
        bottom = hp_pivot_above_so(height) - 0.46 \
            - fields * HP_RED_BOARD_LENGTH
        assert 2.37 <= bottom <= 2.40

    # Every height and both mast constructions use one common wheel geometry.
    # Only the holder changes: the wheel is enclosed by the Gittermast cage and
    # tangent to the Schmalmast roof.  Both cable legs must run continuously
    # from wheel tangent into the mast foot in one rear depth plane.
    for mast in ("gitter", "schmal"):
        layout = hp_head_layout(mast)
        diameter = 2.0 * HP_HEAD_PULLEY_RADIUS
        for nominal_height in HP_PIVOT_HEIGHTS:
            height = hp_pivot_above_so(nominal_height)
            centre = hp_head_pulley_centre(height, mast)
            assert centre[0] == layout["pulley_centre_x"]
            if mast == "gitter":
                assert abs(centre[0]) + HP_HEAD_PULLEY_RADIUS \
                    < layout["support_half_width"]
            else:
                assert abs(centre[0] - HP_HEAD_PULLEY_RADIUS
                           - layout["support_half_width"]) < 1e-12
            assert centre[1] + HP_HEAD_PULLEY_RADIUS \
                < height + layout["above_pivot"]
            cable_runs = hp_head_cable_runs(height, mast)
            assert len(cable_runs) == 2
            assert cable_runs[0][0][0] < centre[0] < cable_runs[1][0][0]
            for cable_top, cable_bottom in cable_runs:
                assert cable_top[0] == cable_bottom[0]
                assert cable_top[1] == centre[1]
                assert cable_bottom[1] == HP_HEAD_CABLE_BOTTOM
                assert cable_top[2] == cable_bottom[2] == HP_HEAD_CABLE_Z
            lead_runs = hp_lantern_lead_paths(nominal_height)
            assert len(lead_runs) == 2
            assert lead_runs[0][0][0] < lead_runs[1][0][0]
            for side, path in zip((-1.0, 1.0), lead_runs):
                lead_top, lead_knee, lead_bottom = path
                assert lead_top[0] == lead_knee[0]
                assert lead_top[1] > height - 0.30
                assert lead_knee[1] == HP_LANTERN_LEAD_KNEE_Y
                assert lead_bottom[1] == HP_LANTERN_LEAD_BOTTOM
                assert lead_bottom[0] == HP_LANTERN_TERMINAL_X \
                    + side * HP_LANTERN_TERMINAL_HALF_SPACING
                assert all(point[2] == HP_LANTERN_LEAD_Z for point in path)
            for lod in range(3):
                pulley_prims = base_prims()
                hp_head_pulley_wheel(pulley_prims, height, mast, lod)
                wheel = pulley_prims["dark"]
                xs = [x for x, _y, _z in wheel.pos]
                ys = [y for _x, y, _z in wheel.pos]
                zs = [z for _x, _y, z in wheel.pos]
                assert 0.945 * diameter <= max(xs) - min(xs) \
                    <= diameter + 1e-9
                assert 0.945 * diameter <= max(ys) - min(ys) \
                    <= diameter + 1e-9
                assert abs(max(zs) - min(zs)
                           - HP_HEAD_PULLEY_DEPTH) < 1e-9
                assert max(xs) - min(xs) > max(zs) - min(zs)

                head = main_static_components(
                    nominal_height, mast, 1, "db_gruen", lod
                )["mast_head"]
                head_ys = [
                    y
                    for primitive in head.values()
                    for _x, y, _z in primitive.pos
                ]
                assert max(head_ys) <= height + layout["above_pivot"] + 1e-9

    # The historical end drive is an offset disc with a shallow Z thickness,
    # not a centred black plate filling the mast bay.
    assert HP_END_DRIVE_CENTRE[0] > HP_END_DRIVE_RADIUS
    for lod in range(3):
        drive_prims = base_prims()
        hp_end_drive_disc(drive_prims, lod)
        drive = drive_prims["dark"]
        xs = [x for x, _y, _z in drive.pos]
        ys = [y for _x, y, _z in drive.pos]
        zs = [z for _x, _y, z in drive.pos]
        diameter = 2.0 * HP_END_DRIVE_RADIUS
        assert min(xs) > 0.0
        assert 0.94 * diameter <= max(xs) - min(xs) <= diameter + 1e-9
        assert 0.94 * diameter <= max(ys) - min(ys) <= diameter + 1e-9
        depth = max(zs) - min(zs)
        assert HP_END_DRIVE_DEPTH - 1e-9 <= depth \
            <= HP_END_DRIVE_DEPTH + 0.010
        assert max(xs) - min(xs) > depth

    # The Hauptsignal blade silhouette is a release gate, not an incidental
    # consequence of primitive order.  It catches regressions in which the
    # counterbalancing root becomes a token mounting tab or the round head
    # changes scale while unrelated details are being edited.
    for lower in (False, True):
        for shortened in (False, True):
            length, width, disc = HP_ARM[(lower, shortened)]
            blade = main_arm_mesh(lower, shortened, False, 0)
            points = [point for primitive in blade.values()
                      for point in primitive.pos]
            if lower:
                extent_length = max(y for _x, y, _z in points) \
                    - min(y for _x, y, _z in points)
                extent_width = max(x for x, _y, _z in points) \
                    - min(x for x, _y, _z in points)
            else:
                extent_length = max(x for x, _y, _z in points) \
                    - min(x for x, _y, _z in points)
                extent_width = max(y for _x, y, _z in points) \
                    - min(y for _x, y, _z in points)
            assert abs(extent_length - length) < 1e-9
            assert extent_width >= disc
            # Rectified prototype views lock the balancing/drive tail at about
            # 1.7 upper and 1.8 lower blade depths.  Keep the two independently
            # reviewed ratios explicit so neither root can regress to the old
            # token mounting tab during an unrelated edit.
            expected_root_ratio = (20.0 / 11.0) if lower else (5.0 / 3.0)
            assert abs(HP_ARM_ROOT[lower] / width
                       - expected_root_ratio) < 1e-9
            face = blade[MAT_RED]
            face_axis_cross = [((y, x) if lower else (x, y))
                               for x, y, _z in face.pos]
            face_width = (max(cross_axis for _axis, cross_axis in face_axis_cross)
                          - min(cross_axis for _axis, cross_axis in face_axis_cross))
            assert abs(face_width - (disc - 0.008)) < 1e-9
            # Vertices at the rectangular root are unaffected by the round
            # head and therefore lock the blade-body width independently.
            root_vertices = [cross_axis for axis, cross_axis in face_axis_cross
                             if axis < -HP_ARM_ROOT[lower] + 0.010]
            assert abs((max(root_vertices) - min(root_vertices))
                       - (width - 0.012)) < 1e-9

            # The front white rectangle is emitted before the circular white
            # insert. Its first 36 box vertices must stop at the outer-disc
            # tangent, leaving the complete red annulus visible. This is a
            # topology guard for the exact visual regression reported in the
            # simulator, not merely a duplicate arithmetic assertion.
            white_front_box = blade[MAT_WHITE].pos[:36]
            longitudinal = [y if lower else x
                            for x, y, _z in white_front_box]
            stripe_end = hp_arm_stripe_end(lower, shortened)
            disc_radius = disc * 0.5
            disc_x = length - HP_ARM_ROOT[lower] - disc_radius
            assert abs(max(longitudinal) - stripe_end) < 1e-9
            assert abs(stripe_end - (disc_x - disc_radius)) < 1e-9
            assert disc_radius - HP_ARM_WHITE_DISC_RADIUS[lower] >= 0.080

    # The photographed upper rear fitting is not decoration on the fixed mast:
    # its compact hammer weight sits on the blade holder and must rotate as one
    # rigid part with that blade.  Lock its reconstructed depth/envelope and
    # real LOD reduction independently from the enamel face.
    assert HP_UPPER_HOLDER_BEND == (-0.135, -0.095)
    assert HP_UPPER_HOLDER_WEIGHT_CENTRE == (-0.305, -0.235)
    assert HP_UPPER_HOLDER_WEIGHT_SPAN == 0.160
    assert HP_UPPER_HOLDER_WEIGHT_WIDTH == 0.075
    balance_counts = []
    for lod in range(3):
        balance = hp_balance_mesh(False, lod)
        assert MAT_RUST not in balance, "Hp balance must not grow orange pseudo-fasteners"
        points = [point for primitive in balance.values() for point in primitive.pos]
        assert max(z for _x, _y, z in points) < -0.20
        assert max(x for x, _y, _z in points) \
            - min(x for x, _y, _z in points) < 0.50
        assert max(y for _x, y, _z in points) \
            - min(y for _x, y, _z in points) < 0.43
        balance_counts.append(sum(len(primitive.idx) for primitive in balance.values()))
    assert balance_counts[0] > balance_counts[1] > balance_counts[2]

    lower_counts = []
    for lod in range(3):
        lower_balance = hp_balance_mesh(True, lod)
        lower_points = [point for primitive in lower_balance.values()
                        for point in primitive.pos]
        lower_size = tuple(max(point[axis] for point in lower_points)
                           - min(point[axis] for point in lower_points)
                           for axis in range(3))
        assert lower_size[0] < 0.18
        assert lower_size[1] < 0.23
        assert lower_size[2] < 0.11
        lower_counts.append(sum(len(primitive.idx)
                                for primitive in lower_balance.values()))
    assert lower_counts[0] > lower_counts[1] > lower_counts[2]

    assert HP_EQUALIZER_HEIGHT_FACTOR == 0.500
    assert HP_EQUALIZER_REACH == 0.320
    assert HP_EQUALIZER_DROP == 0.165
    assert HP_EQUALIZER_WEIGHT_SPAN == 0.145
    assert HP_EQUALIZER_WEIGHT_WIDTH == 0.070
    assert HP_EQUALIZER_SWING_DEGREES == 18.0
    for height in HP_PIVOT_HEIGHTS:
        _upper_y, lower_y = hp_pivot_levels(height)
        equalizer_y = height * HP_EQUALIZER_HEIGHT_FACTOR
        assert 0.17 < equalizer_y < lower_y
    for second in (False, True):
        equalizer_counts = []
        for lod in range(3):
            equalizer = hp_equalizer_mesh(second, lod)
            equalizer_points = [point for primitive in equalizer.values()
                                for point in primitive.pos]
            equalizer_size = tuple(
                max(point[axis] for point in equalizer_points)
                - min(point[axis] for point in equalizer_points)
                for axis in range(3)
            )
            assert 0.42 < equalizer_size[0] < 0.44
            assert 0.28 < equalizer_size[1] < 0.30
            assert equalizer_size[2] < 0.05
            equalizer_counts.append(sum(
                len(primitive.idx) for primitive in equalizer.values()
            ))
        assert equalizer_counts[0] > equalizer_counts[1] > equalizer_counts[2]

    assert HP_SELECTOR_AXIS_DROP == 0.200
    assert HP_SELECTOR_RADIUS == 0.320
    assert HP_SELECTOR_SWING_DEGREES == 60.0
    # Folded rear cheek versus the widest top chord: this is the exact rear
    # clipping regression visible in the in-game screenshots.
    assert HP_SELECTOR_RADIUS - 0.151 > 0.145 + 0.015
    assert 0.0 < HP_RETURN_SPRING_X < HP_SELECTOR_RADIUS
    assert HP_RETURN_SPRING_TOP_DROP < HP_RETURN_SPRING_BOTTOM_DROP
    assert HP_RETURN_SPRING_TURNS >= 12.0
    assert HP_SELECTOR_RING_INNER_RADIUS > LANTERN_GLASS_DIAMETER * 0.5
    assert HP_SELECTOR_RING_OUTER_RADIUS > HP_SELECTOR_RING_INNER_RADIUS
    for lower in (False, True):
        selector = hp_spectacle_mesh(lower, 0)[MAT_DARK]
        selector_x = [x for x, _y, _z in selector.pos]
        selector_y = [y for _x, y, _z in selector.pos]
        # The 320-mm glass orbit keeps the folded lamp back clear of the mast;
        # upper/lower carriers differ slightly because their parked alternate
        # glass lies on opposite sides of the bearing.
        assert 0.47 < max(selector_x) - min(selector_x) < 0.54
        assert 0.48 < max(selector_y) - min(selector_y) < 0.51

    # Lamp bodies are separate reviewable LOD nodes. Their current envelope is
    # a photo reconstruction, not a published production dimension, but it
    # must not silently collapse back into the mast or lose its rear hardware.
    lantern_counts = []
    for lod in range(3):
        lamp_body = hp_lantern_mesh(0.0, lod=lod)
        lamp_points = [point for primitive in lamp_body.values()
                       for point in primitive.pos]
        lamp_size = tuple(max(point[axis] for point in lamp_points)
                          - min(point[axis] for point in lamp_points)
                          for axis in range(3))
        # The body itself remains 302 mm wide; this envelope also includes the
        # now longer, correctly outboard mast bracket.
        assert 0.41 <= lamp_size[0] <= 0.43
        if lod < 2:
            assert MAT_GALVANISED in lamp_body
            assert 0.32 <= lamp_size[1] <= (0.37 if lod == 0 else 0.33)
            # The documented train-facing barrel is unchanged.  The extra
            # rear depth is the separately checked folded service cover,
            # gasket and connector relief visible in the rear photographs.
            assert 0.40 <= lamp_size[2] <= 0.42
        else:
            assert MAT_GALVANISED not in lamp_body
            assert 0.31 <= lamp_size[1] <= 0.33
            assert 0.32 <= lamp_size[2] <= 0.33
        lantern_counts.append(sum(len(primitive.idx)
                                  for primitive in lamp_body.values()))
    assert lantern_counts[0] > lantern_counts[1] > lantern_counts[2]

    # Both selectors have exactly one fixed lamp.  Their own bearing is 200 mm
    # below the blade shaft and the alternate Ø170-mm pane must land on the red
    # pane's optical centre after the independently bound 60-degree movement.
    red_global = hp_lamp_offset()
    for lower, degrees in ((False, HP_SELECTOR_SWING_DEGREES),
                           (True, -HP_SELECTOR_SWING_DEGREES)):
        red, other = hp_selector_holes(lower)
        assert abs(red[0] - HP_SELECTOR_RADIUS) < 1e-12
        assert abs(math.hypot(*other) - HP_SELECTOR_RADIUS) < 1e-12
        a = math.radians(degrees)
        landed = (math.cos(a) * other[0] - math.sin(a) * other[1],
                  math.sin(a) * other[0] + math.cos(a) * other[1])
        assert abs(landed[0] - red[0]) < 0.001
        assert abs(landed[1] - red[1]) < 0.001
        assert abs(red_global[0] - red[0]) < 1e-12
        assert abs(red_global[1] - (red[1] - HP_SELECTOR_AXIS_DROP)) < 1e-12
    layout = vr_night_layout(VR_CENTRE_HEIGHTS[1])
    assert layout["left_green"][0] == -VR_LANTERN_LATERAL_OFFSET
    assert layout["right_green"][0] == VR_LANTERN_LATERAL_OFFSET
    assert layout["left_amber"][1] < layout["right_amber"][1]
    # 220-mm electric carriers leave the photographed ~45-mm clearance to
    # either side of the documented 240-mm wing envelope.
    assert abs(VR_LANTERN_LATERAL_OFFSET
               - VR_LED_LANTERN_CASE[0] * 0.5
               - VR_WING_WIDTH * 0.5 - 0.045) < 1e-12
    assert VR_1944_MAST_WIDTH == 0.100
    assert VR_1944_MAST_DEPTH == 0.250
    assert VR_1944_MAST_PLATE == 0.012
    # The 1944 mast is a continuous I section.  At coarse LOD there are no
    # detail projections, so its exact envelope and the eight web corners can
    # be checked independently of brackets and service hardware.
    mast_height = VR_CENTRE_HEIGHTS[1] - 0.055
    mast_prims = base_prims()
    vr_mast_1944(mast_prims, mast_height, "db_gruen", 2)
    mast_points = mast_prims["steel"].pos
    assert abs(max(x for x, _y, _z in mast_points)
               - min(x for x, _y, _z in mast_points)
               - VR_1944_MAST_WIDTH) < 1e-12
    assert abs(max(z for _x, _y, z in mast_points)
               - min(z for _x, _y, z in mast_points)
               - VR_1944_MAST_DEPTH) < 1e-12
    web_half = VR_1944_MAST_PLATE * 0.5
    web_depth = VR_1944_MAST_DEPTH * 0.5 - VR_1944_MAST_PLATE
    vertices = {(round(x, 6), round(y, 6), round(z, 6))
                for x, y, z in mast_points}
    for x in (-web_half, web_half):
        for y in (0.17, mast_height):
            for z in (-web_depth, web_depth):
                assert (round(x, 6), round(y, 6), round(z, 6)) in vertices
    assert VR_OLD_U_MAST_WIDTH == 0.160
    assert VR_OLD_U_MAST_DEPTH == 0.120
    assert VR_OLD_U_MAST_PLATE == 0.010
    assert VR_OLD_U_MAST_GAP == 0.040
    old_counts = []
    for lod in range(3):
        old_prims = base_prims()
        vr_mast_old_u(old_prims, mast_height, "db_gruen", lod)
        old_counts.append(sum(len(primitive.idx)
                              for primitive in old_prims.values()))
        if lod == 2:
            points = old_prims["steel"].pos
            assert abs(max(x for x, _y, _z in points)
                       - min(x for x, _y, _z in points)
                       - VR_OLD_U_MAST_WIDTH) < 1e-12
            assert abs(max(z for _x, _y, z in points)
                       - min(z for _x, _y, z in points)
                       - VR_OLD_U_MAST_DEPTH) < 1e-12
    assert old_counts[0] > old_counts[1] > old_counts[2]
    assert MAST_BOARD_WIDTH == {"gitter": 0.200, "schmal": 0.100}
    assert SH_CASE_SIZE == 0.700 and SH_CASE_DEPTH == 0.400
    assert SH_LIT_DISC_DIAMETER == 0.560
    assert SH_HIGH_CENTRE == 4.010
    for high, centre in ((False, SH_LOW_CENTRE), (True, SH_HIGH_CENTRE)):
        static = sh_static_prims(centre, high, "db_gruen", 0)
        case = static["black"]
        vertices = {(round(x, 6), round(y, 6), round(z, 6))
                    for x, y, z in case.pos}
        half = SH_CASE_SIZE * 0.5
        depth = SH_CASE_DEPTH * 0.5
        # All eight exact case corners must survive refactors; merely keeping
        # constants above is not enough if the mesh stops using them.
        for x in (-half, half):
            for y in (centre - half, centre + half):
                for z in (-depth, depth):
                    assert (round(x, 6), round(y, 6), round(z, 6)) in vertices
        assert MAT_WHITE not in sh_bar_mesh(0)


def showcase_line_ron(entries):
    """A flat inspection line with every generated model in one transverse row."""
    spacing = 5.50
    centre = (len(entries) - 1) * 0.5
    devices = []
    signals = []
    for index, (model, kind, signal_type) in enumerate(entries):
        lateral = (index - centre) * spacing
        devices.append(
            f'        // {index:02d}: {model}\n'
            f'        (kind: Signal, edge: 0, s: 650.0, facing: Forward, '
            f'lateral_offset: {lateral:.2f}, payload: "(signal:Some({index}))"),'
        )
        next_field = "            next: Some(0),\n" if kind == "Distant" else ""
        guarded = "[0]" if kind == "Main" else "[]"
        signals.append(
            f'        // {index:02d}: {model}\n'
            f'        (\n'
            f'            designation: "S{index + 1}",\n'
            f'            interlocking: "Fsf",\n'
            f'            kind: {kind},\n'
            f'            system: HV,\n'
            f'            device: {index},\n'
            f'{next_field}'
            f'            guarded: {guarded},\n'
            f'            requires_route: true,\n'
            f'            signal_type: Some("example:{signal_type}"),\n'
            f'            model: Some("example:{model}"),\n'
            f'        ),'
        )
    return f'''// Generated by tools/gen_form_signals.py — edit the generator.
// All {len(entries)} full-size DB form-signal models stand in one transverse row.
// The number comments map each placement to the signal-model catalogue name.
(
    name: "DB-Formsignale – Maß- und Materialschau",
    year: Some(2026),
    fictional: true,
    geoid_offset: 46.0,
    nodes: [Buffer, Buffer],
    edges: [(
        from: 0,
        to: 1,
        start: Geo(point: (lat: 52.0, lon: 10.0, height: 100.0), heading_deg: 90.0),
        segments: [(len: 1200.0, k0: 0.0, dk: 0.0)],
        grade: [(0.0, 0.0)],
        speed: [(0.0, 40.0)],
        track_type: [(0.0, "example:altbau")],
        electrification: [(0.0, "none")],
    )],
    devices: [
{chr(10).join(devices)}
    ],
    yards: [
        (name: "Besucherstand", kind: Portal, edge: 0, s: 250.0,
         facing: Forward, length: 120.0),
    ],
    sections: [(edges: [0])],
    signals: [
{chr(10).join(signals)}
    ],
)
'''


def write_showcase_cycle():
    """Demo-only signal types: advance every presentation every ten seconds."""
    SIGNALS.mkdir(parents=True, exist_ok=True)
    SCRIPTS.mkdir(parents=True, exist_ok=True)
    types = {
        "formsignal_demo_hp01": "main: Some(Stop)",
        # Base aspects are internal type markers read by the shared script.
        "formsignal_demo_hp02": "main: Some(ProceedSlow)",
        "formsignal_demo_hp02_gekuppelt": "main: Some(Proceed)",
        "formsignal_demo_vr01": "distant: Some(ExpectStop)",
        "formsignal_demo_vr012": "distant: Some(ExpectSlow)",
        "formsignal_demo_sh": "shunt: Some(Stop)",
    }
    for name, marker in types.items():
        write_model(SIGNALS / f"{name}.ron", f'''(
    system: HV,
    rules: [(when: (), show: ({marker}), lamps: [])],
    script: Some("example:formsignal_showcase_cycle"),
    model: None,
    tags: ["showcase", "ten-second-cycle", "semaphore"],
)
''')
    write_model(SIGNALS / "formsignal_demo_ne2.ron", '''(
    system: HV,
    rules: [(when: (), show: (distant: Some(ExpectStop)), lamps: [])],
    script: None,
    model: None,
    tags: ["showcase", "static", "ne2"],
)
''')
    (SCRIPTS / "formsignal_showcase_cycle.lua").write_text('''-- Generated by tools/gen_form_signals.py — edit the generator.
-- Every demo signal advances synchronously at t = 10, 20, 30 … seconds.
local M = {}
local STEP = 10.0

local function phase(ctx, count)
  return math.floor((ctx.time + 0.000001) / STEP) % count
end

function M.aspect(ctx)
  if ctx.main == "stop" then
    if phase(ctx, 2) == 0 then
      return { main = "stop", lamps = { "lamp_red" } }
    end
    return { main = "proceed", lamps = { "fluegel1", "lamp_green" } }
  end

  if ctx.main == "proceed_slow" then
    local p = phase(ctx, 3)
    if p == 0 then
      return { main = "stop", lamps = { "lamp_red" } }
    elseif p == 1 then
      return { main = "proceed", lamps = { "fluegel1", "lamp_green" } }
    end
    return { main = "proceed_slow", speed = 40.0,
             lamps = { "fluegel1", "fluegel2", "lamp_green", "lamp_yellow" } }
  end

  -- A mechanically coupled two-arm signal has only Hp 0 and Hp 2. One
  -- channel drives both blades and spectacles, so an intermediate Hp 1
  -- command cannot create a contradictory blade position.
  if ctx.main == "proceed" then
    if phase(ctx, 2) == 0 then
      return { main = "stop", lamps = { "lamp_red" } }
    end
    return { main = "proceed_slow", speed = 40.0,
             lamps = { "fluegel_gekuppelt", "lamp_green", "lamp_yellow" } }
  end

  if ctx.distant == "expect_stop" then
    if phase(ctx, 2) == 0 then
      return { distant = "expect_stop", lamps = { "vr0_licht" } }
    end
    return { distant = "expect_proceed",
             lamps = { "scheibe_weg", "vr1_licht",
                       "vr_blende_links_gruen", "vr_blende_rechts_gruen" } }
  end

  if ctx.distant == "expect_slow" then
    local p = phase(ctx, 3)
    if p == 0 then
      return { distant = "expect_stop", lamps = { "vr0_licht" } }
    elseif p == 1 then
      return { distant = "expect_proceed",
               lamps = { "scheibe_weg", "vr1_licht",
                         "vr_blende_links_gruen", "vr_blende_rechts_gruen" } }
    end
    return { distant = "expect_slow",
             lamps = { "vr2_fluegel", "vr2_licht",
                       "vr_blende_rechts_gruen" } }
  end

  if ctx.shunt == "stop" then
    if phase(ctx, 2) == 0 then
      return { shunt = "stop", lamps = { "sh_white" } }
    end
    return { shunt = "proceed",
             lamps = { "sperrscheibe_frei", "sh_white" } }
  end

  return nil
end

return M
''', encoding="utf-8", newline="\n")


def catalogue_model_specs():
    """Return every generated model and the parameters needed to rebuild it.

    The full catalogue intentionally remains the release gate. Interactive
    signal work, however, must not rewrite nearly two hundred multi-megabyte
    glTF files merely to inspect one blade or lamp. This descriptor catalogue
    lets ``--only`` rebuild exactly the canonical model being reviewed while
    using the same builders and validators as the full run.
    """
    specs = {}

    def add(name, kind, **values):
        assert name not in specs, name
        specs[name] = {"kind": kind, **values}

    for scheme in ("db_gruen", "eisengrau"):
        scheme_suffix = "" if scheme == "db_gruen" else f"_{scheme}"
        for height in HP_PIVOT_HEIGHTS:
            for mast in ("gitter", "schmal"):
                for arms in (1, 2):
                    for shortened in (False, True):
                        short_suffix = "_kurz" if shortened else ""
                        name = (
                            f"form_hp_{height:g}m_{mast}_{arms}fl"
                            f"{short_suffix}{scheme_suffix}"
                        )
                        values = dict(
                            height=height,
                            mast=mast,
                            arms=arms,
                            shortened=shortened,
                            scheme=scheme,
                            negative=False,
                            historic=False,
                        )
                        add(name, "hp", coupled=False, **values)
                        if arms == 2:
                            add(f"{name}_gekuppelt", "hp", coupled=True, **values)

    for arms in (1, 2):
        for scheme, negative in (("altanstrich", False), ("eisengrau", True)):
            suffix = f"_{scheme}" + ("_negativ" if negative else "")
            name = f"form_hp_8m_gitter_{arms}fl{suffix}"
            values = dict(
                height=8.0,
                mast="gitter",
                arms=arms,
                shortened=False,
                scheme=scheme,
                negative=negative,
                historic=True,
            )
            add(name, "hp", coupled=False, **values)
            if arms == 2:
                add(f"{name}_gekuppelt", "hp", coupled=True, **values)

    for scheme in ("db_gruen", "eisengrau"):
        for centre in VR_CENTRE_HEIGHTS:
            for aspects in (2, 3):
                constructions = [
                    ("led", "drahtzug", "1944"),
                    ("gas", "drahtzug", "1944"),
                    ("led", "elektro", "1944"),
                    (
                        ("led", "drahtzug", "alt_u")
                        if aspects == 2
                        else ("led", "elektro", "alt_u")
                    ),
                ]
                for lighting, drive, mast_style in constructions:
                    for board_mode in ("am_mast", "frei"):
                        suffixes = []
                        if scheme != "db_gruen":
                            suffixes.append(scheme)
                        if lighting != "led":
                            suffixes.append(lighting)
                        if drive != "drahtzug":
                            suffixes.append("elektroantrieb")
                        if mast_style != "1944":
                            suffixes.append("altmast")
                        if board_mode != "am_mast":
                            suffixes.append("ohne_ne2")
                        variant = "" if not suffixes else "_" + "_".join(suffixes)
                        stem = str(centre).replace(".", "_")
                        add(
                            f"form_vr_{stem}m_{aspects}begr{variant}",
                            "vr",
                            centre=centre,
                            aspects=aspects,
                            scheme=scheme,
                            lighting=lighting,
                            drive=drive,
                            mast_style=mast_style,
                            board_mode=board_mode,
                        )

    for scheme in ("db_gruen", "eisengrau"):
        scheme_suffix = "" if scheme == "db_gruen" else f"_{scheme}"
        for size in ("hoch", "niedrig"):
            add(
                f"form_ne2_frei_{size}{scheme_suffix}",
                "ne2",
                size=size,
                scheme=scheme,
            )
        for high in (False, True):
            add(
                f"form_sh_{'hoch' if high else 'niedrig'}{scheme_suffix}",
                "sh",
                high=high,
                scheme=scheme,
            )

    # 188 geometry variants plus the 42 two-arm coupled model bindings.
    assert len(specs) == 230
    return specs


def validate_generated_model_ron(model):
    """Keep the conspicuous direction/drive regressions in the fast path."""
    ron = (MODELS / f"{model}.ron").read_text(encoding="utf-8")
    if model.startswith("form_vr_"):
        assert "scheibe_weg" not in ron or "degrees: -90.0" in ron
        assert "vr2_fluegel" not in ron or "degrees: 45.0" in ron
        assert "vr_blende_links_gruen" in ron
        assert "farbblende_left_LOD0" in ron and "degrees: 180.0" in ron
        assert "vr_blende_rechts_gruen" in ron
        assert "farbblende_right_LOD0" in ron and "degrees: -180.0" in ron
    if model.startswith("form_hp_") and not model.endswith("_gekuppelt"):
        assert "rebound: 0.36" in ron
        assert "fluegel2" not in ron or "rebound: 0.34" in ron
        assert ron.count('node: "gewicht1_LOD') == len(HP_LODS)
        assert ron.count('node: "gewicht_ausgleich1_LOD') == len(HP_LODS)
        assert ('node: "gewicht2_LOD' not in ron) == ("_1fl" in model)
        assert ('node: "gewicht_ausgleich2_LOD' not in ron) == ("_1fl" in model)
        if "_2fl" in model:
            assert ron.count('node: "gewicht2_LOD') == len(HP_LODS)
            assert ron.count('node: "gewicht_ausgleich2_LOD') == len(HP_LODS)
        assert 'node: "blende1_LOD0"' in ron and "degrees: 60.0" in ron
        assert "blende2" not in ron or "degrees: -60.0" in ron
        assert "fluegel_gekuppelt" not in ron
    if model.endswith("_gekuppelt"):
        assert ron.count('lamp: "fluegel_gekuppelt"') == 8 * len(HP_LODS)
        assert 'lamp: "fluegel1"' not in ron
        assert 'lamp: "fluegel2"' not in ron
        assert "fall_seconds: 0.80, rebound: 0.35" in ron


def generate_one(model):
    """Rebuild and validate one canonical review model, without the showcase."""
    specs = catalogue_model_specs()
    try:
        spec = specs[model]
    except KeyError as error:
        raise SystemExit(
            f"unknown generated form-signal model {model!r}; use --list-models"
        ) from error

    MODELS.mkdir(parents=True, exist_ok=True)
    kind = spec["kind"]
    scheme = spec["scheme"]
    scheme_tag = PAINT_SCHEMES[scheme]["label"]

    if kind == "hp":
        height = spec["height"]
        mast = spec["mast"]
        arms = spec["arms"]
        shortened = spec["shortened"]
        negative = spec["negative"]
        asset = build_main(height, mast, arms, shortened, scheme, negative)
        if spec["historic"]:
            tags = [
                "semaphore", "main-signal", "hv", f"{height:g}m",
                f"{arms}-arm", f"{mast}-mast", scheme, scheme_tag,
                "negative-blade" if negative else "historic-mast-paint",
                "lod", "pbr", "weathered",
            ]
        else:
            tags = [
                "semaphore", "main-signal", "hv", f"{height:g}m",
                f"{arms}-arm", f"{mast}-mast",
                "short-arm" if shortened else "long-arm", scheme,
                scheme_tag, "lod", "pbr", "weathered",
            ]
            if height == 14.0:
                tags.append("historical-special-height")
        coupled = spec["coupled"]
        if coupled:
            tags += ["coupled", "hp0-hp2", "shared-mechanical-drive"]
        write_model(
            MODELS / f"{model}.ron",
            main_model_ron(asset, arms, tags, coupled=coupled),
        )
        required = []
        for level, _distance in HP_LODS:
            suffix = f"_LOD{level}"
            required += [
                *(f"{stem}{suffix}" for stem in hp_static_stems(scheme)),
                f"fluegel1{suffix}",
                f"gewicht1{suffix}", f"gewicht_ausgleich1{suffix}",
                f"blende1{suffix}",
                f"laterne1{suffix}", f"lamp_red{suffix}",
                f"lamp_green{suffix}",
            ]
            if arms == 2:
                required += [
                    f"fluegel2{suffix}", f"gewicht2{suffix}",
                    f"gewicht_ausgleich2{suffix}",
                    f"blende2{suffix}", f"laterne2{suffix}",
                    f"lamp_yellow{suffix}",
                ]
        lod_stems = [
            *HP_REDUCED_LOD_STEMS,
            "fluegel1", "gewicht1", "gewicht_ausgleich1",
            "blende1", "laterne1",
        ]
        if arms == 2:
            lod_stems += ["fluegel2", "gewicht2", "gewicht_ausgleich2",
                          "blende2", "laterne2"]
        validate_asset(
            asset,
            hp_pivot_above_so(height),
            required,
            scheme,
            lod_stems=tuple(lod_stems),
        )

    elif kind == "vr":
        centre = spec["centre"]
        aspects = spec["aspects"]
        lighting = spec["lighting"]
        drive = spec["drive"]
        mast_style = spec["mast_style"]
        board_mode = spec["board_mode"]
        asset = build_distant(
            centre, aspects, scheme, lighting, board_mode, drive, mast_style
        )
        tags = [
            "semaphore", "distant-signal", "hv", f"{centre:.2f}m",
            f"{aspects}-aspect", "vorsignalmast",
            "double-t-mast-1944" if mast_style == "1944" else "old-split-u-mast",
            scheme, scheme_tag, lighting,
            "mechanical-wire-drive" if drive == "drahtzug" else "electric-drive",
            "ne2-on-mast" if board_mode == "am_mast" else "separate-ne2",
            "lod", "pbr", "weathered",
        ]
        if drive == "elektro":
            tags.append("siemens-fahrsperrenantrieb")
            if aspects == 3:
                tags.append("auskuppelaufsatz")
        write_model(
            MODELS / f"{model}.ron",
            distant_model_ron(asset, aspects, lighting, tags),
        )
        required = []
        for level, _distance in VR_LODS:
            suffix = f"_LOD{level}"
            required += [
                f"mast{suffix}", f"antrieb{suffix}", f"scheibe{suffix}",
                f"vr_yellow{suffix}", f"vr_green{suffix}",
                f"vr2_yellow{suffix}", f"vr2_green{suffix}",
                f"farbblende_left{suffix}", f"farbblende_right{suffix}",
            ]
            if aspects == 3:
                required.append(f"vorsignalfluegel{suffix}")
            if lighting == "gas":
                required.append(f"gas_cartridges{suffix}")
        validate_asset(
            asset,
            required_nodes=required,
            expected_scheme=scheme,
            expected_ne2_size="hoch" if board_mode == "am_mast" else None,
            lod_stems=("mast", "antrieb", "scheibe"),
            expect_gas=lighting == "gas",
            expected_vr_centre=centre,
        )

    elif kind == "ne2":
        size = spec["size"]
        asset = build_ne2_standalone(size, scheme)
        tags = [
            "distant-board", "ne2", "freestanding", size, scheme,
            scheme_tag, "lod", "normal-mapped", "pbr", "weathered",
        ]
        write_model(MODELS / f"{model}.ron", ne2_model_ron(asset, tags))
        validate_asset(
            asset,
            required_nodes=[f"tafel_LOD{level}" for level, _distance in VR_LODS],
            expected_scheme=scheme,
            expected_ne2_size=size,
            lod_stems=("tafel",),
        )

    elif kind == "sh":
        high = spec["high"]
        asset = build_sh(high, scheme)
        tags = [
            "semaphore", "shunting", "sperrsignal",
            "high" if high else "dwarf", scheme, scheme_tag,
            "plain-channel-mast", "lod", "pbr", "weathered",
        ]
        write_model(MODELS / f"{model}.ron", sh_model_ron(asset, tags))
        required = [
            f"{stem}_LOD{level}"
            for level, _distance in SIGNAL_LODS
            for stem in ("traeger", "sperrscheibe", "sh_white")
        ]
        validate_asset(
            asset,
            required_nodes=required,
            expected_scheme=scheme,
            lod_stems=("traeger", "sperrscheibe", "sh_white"),
        )
    else:  # pragma: no cover - catalogue construction is exhaustive.
        raise AssertionError(kind)

    validate_generated_model_ron(model)
    return model


def generate():
    validate_dimensions()
    MODELS.mkdir(parents=True, exist_ok=True)
    generated = []
    coupled_models = []
    showcase = []

    # Main-signal catalogue. Keep this first in the showcase so the green
    # distant-signal family begins near the visitor camera in the row centre.
    for scheme in ("db_gruen", "eisengrau"):
        scheme_suffix = "" if scheme == "db_gruen" else f"_{scheme}"
        scheme_tag = PAINT_SCHEMES[scheme]["label"]
        for height in HP_PIVOT_HEIGHTS:
            for mast in ("gitter", "schmal"):
                for arms in (1, 2):
                    for shortened in (False, True):
                        asset = build_main(height, mast, arms, shortened, scheme)
                        suffix = "_kurz" if shortened else ""
                        model = f"form_hp_{height:g}m_{mast}_{arms}fl{suffix}{scheme_suffix}"
                        tags = ["semaphore", "main-signal", "hv", f"{height:g}m",
                                f"{arms}-arm", f"{mast}-mast",
                                "short-arm" if shortened else "long-arm", scheme,
                                scheme_tag, "lod", "pbr", "weathered"]
                        if height == 14.0:
                            tags.append("historical-special-height")
                        write_model(MODELS / f"{model}.ron", main_model_ron(asset, arms, tags))
                        if arms == 2:
                            coupled_model = f"{model}_gekuppelt"
                            write_model(
                                MODELS / f"{coupled_model}.ron",
                                main_model_ron(
                                    asset, arms,
                                    tags + ["coupled", "hp0-hp2",
                                            "shared-mechanical-drive"],
                                    coupled=True,
                                ),
                            )
                            coupled_models.append(coupled_model)
                        required = []
                        for level, _distance in HP_LODS:
                            lod_suffix = f"_LOD{level}"
                            required += [
                                *(f"{stem}{lod_suffix}"
                                  for stem in hp_static_stems(scheme)),
                                f"fluegel1{lod_suffix}",
                                f"gewicht1{lod_suffix}",
                                f"gewicht_ausgleich1{lod_suffix}",
                                f"blende1{lod_suffix}",
                                f"laterne1{lod_suffix}",
                                f"lamp_red{lod_suffix}",
                                f"lamp_green{lod_suffix}",
                            ]
                            if arms == 2:
                                required += [
                                    f"fluegel2{lod_suffix}",
                                    f"gewicht2{lod_suffix}",
                                    f"gewicht_ausgleich2{lod_suffix}",
                                    f"blende2{lod_suffix}",
                                    f"laterne2{lod_suffix}",
                                    f"lamp_yellow{lod_suffix}",
                                ]
                        lod_stems = [
                            *HP_REDUCED_LOD_STEMS,
                            "fluegel1", "gewicht1", "gewicht_ausgleich1",
                            "blende1", "laterne1",
                        ]
                        if arms == 2:
                            lod_stems += [
                                "fluegel2", "gewicht2", "gewicht_ausgleich2",
                                "blende2", "laterne2",
                            ]
                        validate_asset(asset, hp_pivot_above_so(height), required, scheme,
                                       lod_stems=tuple(lod_stems))
                        generated.append(model)
                        signal_type = ("formsignal_demo_hp02" if arms == 2
                                       else "formsignal_demo_hp01")
                        showcase.append((model, "Main", signal_type))
                        # Two canonical placements make the mechanically
                        # coupled Hp 0/Hp 2 behaviour visible without filling
                        # the review line with geometry-identical duplicates.
                        if (arms == 2 and height == 8.0 and not shortened
                                and scheme == "db_gruen"):
                            showcase.append((
                                coupled_model, "Main",
                                "formsignal_demo_hp02_gekuppelt",
                            ))

    # Distant signals use only the documented dedicated narrow Vorsignalmast.
    # Paint, night-sign technology and the Ne 2 arrangement remain configurable.
    for scheme in ("db_gruen", "eisengrau"):
        scheme_tag = PAINT_SCHEMES[scheme]["label"]
        for centre in VR_CENTRE_HEIGHTS:
            for aspects in (2, 3):
                # Drive and night-sign technology are separate construction
                # axes.  Keep the existing LED/draught-wire model names as the
                # compatibility default, add the preserved Siemens electrical
                # drive explicitly, and retain gas only on the documented
                # wire-driven construction until a contrary source is found.
                construction_variants = [
                    ("led", "drahtzug", "1944"),
                    ("gas", "drahtzug", "1944"),
                    ("led", "elektro", "1944"),
                ]
                # Do not fabricate every possible cross product for the old
                # split-U mast.  The surviving Germersheim two-aspect example
                # documents a wire-driven/electrically lit conversion; J35-326
                # documents the three-aspect Siemens-driven construction.
                construction_variants.append(
                    ("led", "drahtzug", "alt_u") if aspects == 2
                    else ("led", "elektro", "alt_u"))
                for lighting, drive, mast_style in construction_variants:
                    for board_mode in ("am_mast", "frei"):
                        asset = build_distant(
                            centre, aspects, scheme, lighting, board_mode,
                            drive, mast_style)
                        stem = str(centre).replace(".", "_")
                        suffixes = []
                        if scheme != "db_gruen":
                            suffixes.append(scheme)
                        if lighting != "led":
                            suffixes.append(lighting)
                        if drive != "drahtzug":
                            suffixes.append("elektroantrieb")
                        if mast_style != "1944":
                            suffixes.append("altmast")
                        if board_mode != "am_mast":
                            suffixes.append("ohne_ne2")
                        variant = "" if not suffixes else "_" + "_".join(suffixes)
                        model = f"form_vr_{stem}m_{aspects}begr{variant}"
                        tags = [
                            "semaphore", "distant-signal", "hv",
                            f"{centre:.2f}m", f"{aspects}-aspect",
                            "vorsignalmast",
                            "double-t-mast-1944" if mast_style == "1944"
                            else "old-split-u-mast",
                            scheme, scheme_tag, lighting,
                            "mechanical-wire-drive" if drive == "drahtzug"
                            else "electric-drive",
                            "ne2-on-mast" if board_mode == "am_mast"
                            else "separate-ne2",
                            "lod", "pbr", "weathered",
                        ]
                        if drive == "elektro":
                            tags.append("siemens-fahrsperrenantrieb")
                            if aspects == 3:
                                tags.append("auskuppelaufsatz")
                        write_model(MODELS / f"{model}.ron",
                                    distant_model_ron(asset, aspects, lighting, tags))
                        required = []
                        for level, _distance in VR_LODS:
                            lod_suffix = f"_LOD{level}"
                            required += [
                                f"mast{lod_suffix}", f"antrieb{lod_suffix}",
                                f"scheibe{lod_suffix}",
                                f"vr_yellow{lod_suffix}",
                                f"vr_green{lod_suffix}",
                                f"vr2_yellow{lod_suffix}",
                                f"vr2_green{lod_suffix}",
                            ]
                            if aspects == 3:
                                required.append(f"vorsignalfluegel{lod_suffix}")
                            required += [
                                f"farbblende_left{lod_suffix}",
                                f"farbblende_right{lod_suffix}",
                            ]
                            if lighting == "gas":
                                required.append(f"gas_cartridges{lod_suffix}")
                        validate_asset(
                            asset, required_nodes=required,
                            expected_scheme=scheme,
                            expected_ne2_size=("hoch" if board_mode == "am_mast"
                                               else None),
                            lod_stems=("mast", "antrieb", "scheibe"),
                            expect_gas=lighting == "gas",
                            expected_vr_centre=centre,
                        )
                        generated.append(model)
                        signal_type = ("formsignal_demo_vr012" if aspects == 3
                                       else "formsignal_demo_vr01")
                        showcase.append((model, "Distant", signal_type))

    # Freestanding Ne 2 boards: both dimensions from DB S 525.1 and both mast
    # paint eras. They can be placed independently beside a *_ohne_ne2 signal.
    for scheme in ("db_gruen", "eisengrau"):
        scheme_suffix = "" if scheme == "db_gruen" else f"_{scheme}"
        scheme_tag = PAINT_SCHEMES[scheme]["label"]
        for size in ("hoch", "niedrig"):
            asset = build_ne2_standalone(size, scheme)
            model = f"form_ne2_frei_{size}{scheme_suffix}"
            tags = ["distant-board", "ne2", "freestanding", size, scheme,
                    scheme_tag, "lod", "normal-mapped", "pbr", "weathered"]
            write_model(MODELS / f"{model}.ron", ne2_model_ron(asset, tags))
            required = [f"tafel_LOD{level}" for level, _distance in VR_LODS]
            validate_asset(asset, required_nodes=required,
                           expected_scheme=scheme, expected_ne2_size=size,
                           lod_stems=("tafel",))
            generated.append(model)
            showcase.append((model, "Distant", "formsignal_demo_ne2"))

    for scheme in ("db_gruen", "eisengrau"):
        scheme_suffix = "" if scheme == "db_gruen" else f"_{scheme}"
        scheme_tag = PAINT_SCHEMES[scheme]["label"]
        for high in (False, True):
            asset = build_sh(high, scheme)
            model = f"form_sh_{'hoch' if high else 'niedrig'}{scheme_suffix}"
            tags = ["semaphore", "shunting", "sperrsignal",
                    "high" if high else "dwarf", scheme, scheme_tag,
                    "plain-channel-mast", "lod", "pbr", "weathered"]
            write_model(MODELS / f"{model}.ron", sh_model_ron(asset, tags))
            required = [
                f"{stem}_LOD{level}"
                for level, _distance in SIGNAL_LODS
                for stem in ("traeger", "sperrscheibe", "sh_white")
            ]
            validate_asset(asset, required_nodes=required,
                           expected_scheme=scheme,
                           lod_stems=("traeger", "sperrscheibe", "sh_white"))
            generated.append(model)
            showcase.append((model, "Shunting", "formsignal_demo_sh"))

    # The old painted mast predates the separate mast board.  It is included in
    # the documented regular 8 m Gittermast forms; rare negative blades are kept
    # separate and paired with the historically appropriate iron-grey mast.
    for arms in (1, 2):
        for scheme, negative in (("altanstrich", False), ("eisengrau", True)):
            asset = build_main(8.0, "gitter", arms, False, scheme, negative)
            suffix = f"_{scheme}" + ("_negativ" if negative else "")
            model = f"form_hp_8m_gitter_{arms}fl{suffix}"
            tags = ["semaphore", "main-signal", "hv", "8m", f"{arms}-arm",
                    "gitter-mast", scheme, PAINT_SCHEMES[scheme]["label"],
                    "negative-blade" if negative else "historic-mast-paint",
                    "lod", "pbr", "weathered"]
            write_model(MODELS / f"{model}.ron", main_model_ron(asset, arms, tags))
            if arms == 2:
                coupled_model = f"{model}_gekuppelt"
                write_model(
                    MODELS / f"{coupled_model}.ron",
                    main_model_ron(
                        asset, arms,
                        tags + ["coupled", "hp0-hp2",
                                "shared-mechanical-drive"],
                        coupled=True,
                    ),
                )
                coupled_models.append(coupled_model)
            required = []
            for level, _distance in HP_LODS:
                lod_suffix = f"_LOD{level}"
                required += [
                    *(f"{stem}{lod_suffix}"
                      for stem in hp_static_stems(scheme)),
                    f"fluegel1{lod_suffix}",
                    f"gewicht1{lod_suffix}",
                    f"gewicht_ausgleich1{lod_suffix}",
                    f"blende1{lod_suffix}", f"laterne1{lod_suffix}",
                    f"lamp_red{lod_suffix}",
                    f"lamp_green{lod_suffix}",
                ]
                if arms == 2:
                    required += [
                        f"fluegel2{lod_suffix}", f"gewicht2{lod_suffix}",
                        f"gewicht_ausgleich2{lod_suffix}",
                        f"blende2{lod_suffix}", f"laterne2{lod_suffix}",
                        f"lamp_yellow{lod_suffix}",
                    ]
            lod_stems = [
                *HP_REDUCED_LOD_STEMS,
                "fluegel1", "gewicht1", "gewicht_ausgleich1",
                "blende1", "laterne1",
            ]
            if arms == 2:
                lod_stems += [
                    "fluegel2", "gewicht2", "gewicht_ausgleich2",
                    "blende2", "laterne2",
                ]
            validate_asset(asset, hp_pivot_above_so(8.0), required, scheme,
                           lod_stems=tuple(lod_stems))
            generated.append(model)
            signal_type = ("formsignal_demo_hp02" if arms == 2
                           else "formsignal_demo_hp01")
            showcase.append((model, "Main", signal_type))
    # Compatibility alias used by early example content.  It must never regress
    # to the old placeholder geometry when the generators are run independently.
    shutil.copyfile(ROOT / "mods/example/assets/sig_form_hp_8m_gitter_2fl.gltf",
                    ROOT / "mods/example/assets/sig_form_hp.gltf")
    write_model(
        MODELS / "form_hp.ron",
        main_model_ron(
            "sig_form_hp.gltf", 2,
            ["semaphore", "mast", "main-signal", "hv", "8m",
             "2-arm", "gitter-mast", "db_gruen", "lod", "pbr",
             "weathered", "compatibility-alias"],
        ),
    )
    write_showcase_cycle()
    (ROOT / "mods/example/lines/formsignal_showcase.ron").write_text(
        showcase_line_ron(showcase), encoding="utf-8", newline="\n")
    assert len(generated) == 188
    assert len(coupled_models) == 42
    assert len(showcase) == 190
    assert set(generated).isdisjoint(coupled_models)
    assert set(generated) | set(coupled_models) == set(catalogue_model_specs()), \
        "fast --only catalogue drifted from the complete generator"
    # Regression guards for the three visually conspicuous motion corrections.
    # These failed in an earlier asset pass while the meshes themselves still
    # validated, so keep them explicit alongside the catalogue-size invariant.
    wing_lod0 = vr_wing_mesh(0)
    assert not wing_lod0[MAT_RUST].pos, "Vr wing must not grow an orange rust stud"
    assert wing_lod0[MAT_DARK].pos, "Vr wing centre axle hardware is missing"
    assert len(wing_lod0[MAT_DARK].idx) >= 1000, \
        "Vr wing's paired edge fasteners are missing"
    for model in generated:
        ron = (MODELS / f"{model}.ron").read_text(encoding="utf-8")
        if model.startswith("form_vr_"):
            assert "scheibe_weg" not in ron or "degrees: -90.0" in ron
            assert "vr2_fluegel" not in ron or "degrees: 45.0" in ron
            assert "vr_blende_links_gruen" in ron
            assert "farbblende_left_LOD0" in ron and "degrees: 180.0" in ron
            assert "vr_blende_rechts_gruen" in ron
            assert "farbblende_right_LOD0" in ron and "degrees: -180.0" in ron
        if model.startswith("form_hp_"):
            assert "rebound: 0.36" in ron
            assert "fluegel2" not in ron or "rebound: 0.34" in ron
            assert ron.count('node: "gewicht1_LOD') == len(HP_LODS)
            assert ron.count('node: "gewicht_ausgleich1_LOD') == len(HP_LODS)
            assert ('node: "gewicht2_LOD' not in ron) == ("_1fl" in model)
            assert ('node: "gewicht_ausgleich2_LOD' not in ron) == ("_1fl" in model)
            if "_2fl" in model:
                assert ron.count('node: "gewicht2_LOD') == len(HP_LODS)
                assert ron.count('node: "gewicht_ausgleich2_LOD') == len(HP_LODS)
            assert 'node: "blende1_LOD0"' in ron and "degrees: 60.0" in ron
            assert "blende2" not in ron or "degrees: -60.0" in ron
            assert "fluegel_gekuppelt" not in ron
    for model in coupled_models:
        ron = (MODELS / f"{model}.ron").read_text(encoding="utf-8")
        # Both blades, colour selectors, shaft-mounted holders and the two
        # equalising levers share the coupled drive at every LOD.
        assert ron.count('lamp: "fluegel_gekuppelt"') == 8 * len(HP_LODS)
        assert ron.count('node: "gewicht1_LOD') == len(HP_LODS)
        assert ron.count('node: "gewicht2_LOD') == len(HP_LODS)
        assert ron.count('node: "gewicht_ausgleich1_LOD') == len(HP_LODS)
        assert ron.count('node: "gewicht_ausgleich2_LOD') == len(HP_LODS)
        assert 'lamp: "fluegel1"' not in ron
        assert 'lamp: "fluegel2"' not in ron
        assert "fall_seconds: 0.80, rebound: 0.35" in ron
    print(
        f"generated and validated {len(generated)} geometric form-signal variants, "
        f"{len(coupled_models)} coupled drive configurations and showcase line"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--only",
        action="append",
        metavar="MODEL",
        help="rebuild one canonical model for interactive preview (repeatable)",
    )
    parser.add_argument(
        "--list-models",
        action="store_true",
        help="list model names accepted by --only",
    )
    args = parser.parse_args()
    if args.list_models:
        print("\n".join(sorted(catalogue_model_specs())))
        return
    if args.only:
        validate_dimensions()
        generated = []
        for model in dict.fromkeys(args.only):
            generated.append(generate_one(model))
        print("generated and validated preview model(s): " + ", ".join(generated))
        return
    generate()


if __name__ == "__main__":
    main()
