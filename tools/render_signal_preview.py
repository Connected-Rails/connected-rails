#!/usr/bin/env python3
"""Headless Blender preview for a generated signal.

Usage:
  blender -b -P tools/render_signal_preview.py -- ASSET OUTPUT [hp0|hp1|hp2|vr0|vr1|vr2|sh0|sh1]
"""

import math
import sys
from pathlib import Path

import bpy
from mathutils import Quaternion, Vector


def main():
    args = sys.argv[sys.argv.index("--") + 1:]
    if len(args) < 2:
        raise SystemExit("ASSET and OUTPUT are required")
    asset, output = map(Path, args[:2])
    aspect = args[2].lower() if len(args) > 2 else "hp0"
    view = args[3].lower() if len(args) > 3 else "front"

    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    bpy.ops.import_scene.gltf(filepath=str(asset.resolve()))

    visible_lamps = {
        "hp0": {"lamp_red"}, "hp1": {"lamp_green"},
        "hp2": {"lamp_green", "lamp_yellow"},
        "vr0": {"vr_yellow"}, "vr1": {"vr_green"},
        "vr2": {"vr2_yellow", "vr2_green"},
        "sh0": set(), "sh1": {"sh_white"},
    }.get(aspect, set())
    all_lamps = {"lamp_red", "lamp_green", "lamp_yellow", "vr_yellow", "vr_green",
                 "vr2_yellow", "vr2_green", "sh_white"}
    for obj in bpy.context.scene.objects:
        # Preview only the close-up geometry when an asset carries several LODs.
        # Lamps and moving components are duplicated per LOD and therefore use
        # their unsuffixed semantic name for aspect selection below.
        if "_LOD" in obj.name and not obj.name.endswith("_LOD0"):
            obj.hide_render = True
            continue
        semantic_name = obj.name.rsplit("_LOD", 1)[0]
        if semantic_name in all_lamps:
            obj.hide_render = semantic_name not in visible_lamps

    rotations = {
        "hp1": (("fluegel1", "Z", 45), ("blende1", "Z", 60)),
        "hp2": (("fluegel1", "Z", 45), ("blende1", "Z", 60),
                ("fluegel2", "Z", -45), ("blende2", "Z", -60)),
        "vr1": (("scheibe", "X", -90),
                ("farbblende_left", "Z", 180),
                ("farbblende_right", "Z", -180)),
        "vr2": (("vorsignalfluegel", "Z", 45),
                ("farbblende_right", "Z", -180)),
        "sh1": (("sperrscheibe", "Z", 45),),
    }
    # glTF +X/+Y/+Z become Blender +X/+Z/-Y on import.
    axes = {"X": Vector((1, 0, 0)), "Y": Vector((0, 0, 1)), "Z": Vector((0, -1, 0))}
    for name, axis, degrees in rotations.get(aspect, ()):
        for candidate in (name, f"{name}_LOD0"):
            obj = bpy.data.objects.get(candidate)
            if obj:
                obj.rotation_mode = "QUATERNION"
                obj.rotation_quaternion = (Quaternion(axes[axis], math.radians(degrees))
                                           @ obj.rotation_quaternion)

    bpy.context.view_layer.update()
    mesh_objects = [obj for obj in bpy.context.scene.objects if obj.type == "MESH" and not obj.hide_render]
    corners = [obj.matrix_world @ Vector(corner) for obj in mesh_objects for corner in obj.bound_box]
    low = Vector((min(v.x for v in corners), min(v.y for v in corners), min(v.z for v in corners)))
    high = Vector((max(v.x for v in corners), max(v.y for v in corners), max(v.z for v in corners)))
    centre = (low + high) * 0.5
    span = max(high.x - low.x, high.y - low.y, high.z - low.z)

    # Blender converts glTF's Y-up coordinates to its own Z-up convention.
    bpy.ops.mesh.primitive_plane_add(size=max(12.0, span * 2.2), location=(0, 0, low.z - 0.02))
    floor = bpy.context.object
    floor.data.materials.append(bpy.data.materials.new("neutral ground"))
    floor.data.materials[0].diffuse_color = (0.16, 0.18, 0.16, 1)

    bpy.ops.object.light_add(type="AREA", location=(4.5, -4.0, high.z + 3.0))
    bpy.context.object.data.energy = 1100
    bpy.context.object.data.shape = "DISK"
    bpy.context.object.data.size = 5.0
    bpy.context.object.rotation_euler = (math.radians(28), 0, math.radians(145))
    bpy.ops.object.light_add(type="AREA", location=(-3.0, 2.0, centre.z + 2.0))
    bpy.context.object.data.energy = 650
    bpy.context.object.data.size = 3.0
    bpy.context.object.rotation_euler = (math.radians(72), 0, math.radians(-55))

    close = view.endswith("-close")
    rear = view.startswith("rear")
    side = view.startswith("side")
    high_view = view.startswith("high")
    straight = view.startswith("straight")
    if close:
        centre.z = high.z - min(1.25, span * 0.15)
        span = min(span, 3.4)
    if high_view:
        camera_location = (span * 0.72, -span * 1.10,
                           centre.z + span * 0.82)
    elif side:
        camera_location = (span * 1.55, -span * 0.06,
                           centre.z + span * 0.08)
    else:
        camera_y = span * 1.55 if rear else -span * 1.55
        camera_location = (0.0 if straight else span * 0.82, camera_y,
                           centre.z + span * 0.08)
    bpy.ops.object.camera_add(location=camera_location)
    camera = bpy.context.object
    bpy.context.scene.camera = camera
    direction = centre - camera.location
    camera.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    camera.data.lens = 58

    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x = 900
    scene.render.resolution_y = 1100
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.render.filepath = str(output.resolve())
    scene.render.film_transparent = False
    scene.world.color = (0.055, 0.075, 0.10)
    scene.view_settings.look = "AgX - Medium High Contrast"
    bpy.ops.render.render(write_still=True)


if __name__ == "__main__":
    main()
