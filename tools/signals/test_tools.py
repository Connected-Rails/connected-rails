from concurrent.futures import ThreadPoolExecutor
import base64
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import unittest


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "tools"))

import gen_form_signals  # noqa: E402
import preview  # noqa: E402
import review  # noqa: E402
import workbench  # noqa: E402


class PreviewTests(unittest.TestCase):
    def test_single_output_accepts_a_png_or_a_suffixless_directory(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            default = root / "defaults" / "hp2-front-head-lod0-bgneutral.png"
            explicit = root / "capture.png"
            directory = root / "iteration-07"

            self.assertEqual(preview.single_output_path(None, default), default)
            self.assertEqual(preview.single_output_path(explicit, default), explicit)
            self.assertEqual(
                preview.single_output_path(directory, default),
                directory / default.name,
            )
            with self.assertRaises(SystemExit):
                preview.single_output_path(root / "capture.jpg", default)

    def test_aspect_catalogue_distinguishes_constructions(self) -> None:
        self.assertEqual(
            preview.aspects_for("form_hp_8m_gitter_2fl"), ("hp0", "hp1", "hp2")
        )
        self.assertEqual(
            preview.aspects_for("form_hp_8m_gitter_2fl_gekuppelt"), ("hp0", "hp2")
        )
        self.assertEqual(
            preview.aspects_for("form_vr_4_87m_2begr"), ("vr0", "vr1")
        )
        self.assertEqual(
            preview.aspects_for("form_vr_4_87m_3begr"), ("vr0", "vr1", "vr2")
        )

    def test_gltf_fingerprints_separate_geometry_from_shading(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            texture = directory / "surface.png"
            texture.write_bytes(b"first texture")
            document = {
                "scene": 0,
                "scenes": [{"nodes": [0]}],
                "nodes": [{"mesh": 0, "translation": [0, 1, 0]}],
                "meshes": [{"primitives": [{"attributes": {"POSITION": 0},
                                               "material": 0}]}],
                "accessors": [{"bufferView": 0, "componentType": 5126,
                               "count": 1, "type": "VEC3"}],
                "bufferViews": [{"buffer": 0, "byteLength": 12}],
                "buffers": [{"byteLength": 12, "uri": "data:application/octet-stream;base64,"
                             + base64.b64encode(b"geometry123").decode()}],
                "materials": [{"pbrMetallicRoughness": {"roughnessFactor": 0.5}}],
                "images": [{"uri": texture.name}],
                "textures": [{"source": 0}],
                "samplers": [],
            }
            gltf = directory / "signal.gltf"
            gltf.write_text(json.dumps(document), encoding="utf-8")
            before = preview.gltf_fingerprints(gltf)
            texture.write_bytes(b"second texture")
            document["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"] = 0.8
            gltf.write_text(json.dumps(document), encoding="utf-8")
            after = preview.gltf_fingerprints(gltf)

            self.assertEqual(before["geometry_sha256"], after["geometry_sha256"])
            self.assertNotEqual(before["shading_sha256"], after["shading_sha256"])
            self.assertEqual(
                before["node_geometry_sha256"], after["node_geometry_sha256"]
            )
            self.assertNotEqual(
                before["node_shading_sha256"], after["node_shading_sha256"]
            )
            self.assertEqual(preview.gltf_external_dependencies(gltf), [texture])

            document["buffers"][0]["uri"] = (
                "data:application/octet-stream;base64,"
                + base64.b64encode(b"changed-geom").decode()
            )
            gltf.write_text(json.dumps(document), encoding="utf-8")
            changed_geometry = preview.gltf_fingerprints(gltf)
            self.assertNotEqual(
                after["node_geometry_sha256"],
                changed_geometry["node_geometry_sha256"],
            )

    def test_model_fingerprints_separate_node_bindings_from_static_config(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            model = Path(temporary) / "signal.ron"
            model.write_text(
                '(\n  motions: [\n'
                '    (lamp: "arm", node: "arm_LOD0", seconds: 1.0),\n'
                '    (lamp: "disc", node: "disc_LOD0", seconds: 2.0),\n'
                '  ],\n  tags: ["fixed"],\n)\n',
                encoding="utf-8",
            )
            before = preview.model_fingerprints(model)
            model.write_text(
                model.read_text(encoding="utf-8").replace("seconds: 1.0", "seconds: 1.5"),
                encoding="utf-8",
            )
            binding_edit = preview.model_fingerprints(model)
            self.assertEqual(before["static_sha256"], binding_edit["static_sha256"])
            self.assertNotEqual(
                before["node_binding_sha256"]["arm_LOD0"],
                binding_edit["node_binding_sha256"]["arm_LOD0"],
            )
            self.assertEqual(
                before["node_binding_sha256"]["disc_LOD0"],
                binding_edit["node_binding_sha256"]["disc_LOD0"],
            )

    def test_component_guard_enforces_node_and_edit_domain(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            guard = Path(temporary) / "guard.json"
            expected = {
                "model": "mods/example/signal_models/test.ron",
                "model_fingerprints": {
                    "static_sha256": "static",
                    "node_binding_sha256": {
                        "arm_LOD0": "arm-binding-a",
                        "mast_LOD0": "mast-binding-a",
                    },
                },
                "gltf_fingerprints": {
                    "mods/example/assets/test.gltf": {
                        "node_geometry_sha256": {
                            "arm_LOD0": "arm-geometry-a",
                            "mast_LOD0": "mast-geometry-a",
                        },
                        "node_shading_sha256": {
                            "arm_LOD0": "arm-shading-a",
                            "mast_LOD0": "mast-shading-a",
                        },
                    }
                },
            }
            guard.write_text(json.dumps(expected), encoding="utf-8")

            geometry_edit = json.loads(json.dumps(expected))
            geometry_edit["gltf_fingerprints"]["mods/example/assets/test.gltf"][
                "node_geometry_sha256"
            ]["arm_LOD0"] = "arm-geometry-b"
            preview.verify_component_guard(
                geometry_edit, guard, ("arm",), ("geometry",)
            )

            wrong_domain = json.loads(json.dumps(geometry_edit))
            wrong_domain["gltf_fingerprints"]["mods/example/assets/test.gltf"][
                "node_shading_sha256"
            ]["arm_LOD0"] = "arm-shading-b"
            with self.assertRaises(SystemExit):
                preview.verify_component_guard(
                    wrong_domain, guard, ("arm",), ("geometry",)
                )

            collateral = json.loads(json.dumps(geometry_edit))
            collateral["gltf_fingerprints"]["mods/example/assets/test.gltf"][
                "node_geometry_sha256"
            ]["mast_LOD0"] = "mast-geometry-b"
            with self.assertRaises(SystemExit):
                preview.verify_component_guard(
                    collateral, guard, ("arm",), ("geometry",)
                )

            binding_edit = json.loads(json.dumps(expected))
            binding_edit["model_fingerprints"]["node_binding_sha256"][
                "arm_LOD0"
            ] = "arm-binding-b"
            preview.verify_component_guard(
                binding_edit, guard, ("arm",), ("binding",)
            )
            with self.assertRaises(SystemExit):
                preview.verify_component_guard(
                    binding_edit, guard, ("arm",), ("geometry",)
                )

            static_edit = json.loads(json.dumps(expected))
            static_edit["model_fingerprints"]["static_sha256"] = "changed"
            with self.assertRaises(SystemExit):
                preview.verify_component_guard(
                    static_edit, guard, ("arm",), ("geometry", "shading", "binding")
                )

    def test_fingerprint_guards_are_domain_specific_and_fail_closed(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            manifest = Path(temporary) / "manifest.json"
            expected = {
                "model": "mods/example/signal_models/test.ron",
                "gltf_fingerprints": {
                    "mods/example/assets/test.gltf": {
                        "geometry_sha256": "geometry-a",
                        "shading_sha256": "shading-a",
                    }
                },
            }
            manifest.write_text(json.dumps(expected), encoding="utf-8")
            changed_shading = json.loads(json.dumps(expected))
            changed_shading["gltf_fingerprints"]["mods/example/assets/test.gltf"][
                "shading_sha256"
            ] = "shading-b"

            preview.verify_fingerprint_guard(
                changed_shading, manifest, "geometry_sha256"
            )
            with self.assertRaises(SystemExit):
                preview.verify_fingerprint_guard(
                    changed_shading, manifest, "shading_sha256"
                )

            wrong_model = json.loads(json.dumps(expected))
            wrong_model["model"] = "another-model.ron"
            with self.assertRaises(SystemExit):
                preview.verify_fingerprint_guard(
                    wrong_model, manifest, "geometry_sha256"
                )

    def test_unrelated_geometry_guard_allows_only_named_node_prefixes(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            manifest = Path(temporary) / "manifest.json"
            expected = {
                "model": "mods/example/signal_models/test.ron",
                "gltf_fingerprints": {
                    "mods/example/assets/test.gltf": {
                        "node_geometry_sha256": {
                            "fluegel1_LOD0": "blade-a",
                            "laterne1_LOD0": "lamp-a",
                            "laterne2_LOD0": "lamp-a",
                            "mast_LOD0": "mast-a",
                        }
                    }
                },
            }
            manifest.write_text(json.dumps(expected), encoding="utf-8")
            lamp_edit = json.loads(json.dumps(expected))
            nodes = lamp_edit["gltf_fingerprints"]["mods/example/assets/test.gltf"][
                "node_geometry_sha256"
            ]
            nodes["laterne1_LOD0"] = "lamp-b"
            nodes["laterne2_LOD0"] = "lamp-b"
            preview.verify_unrelated_geometry_guard(
                lamp_edit, manifest, ("laterne1", "laterne2")
            )

            nodes["fluegel1_LOD0"] = "blade-b"
            with self.assertRaises(SystemExit):
                preview.verify_unrelated_geometry_guard(
                    lamp_edit, manifest, ("laterne1", "laterne2")
                )

    @unittest.skipUnless(shutil.which("magick"), "ImageMagick is not installed")
    def test_parallel_contact_sheets_never_share_tiles(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source = directory / "source.png"
            subprocess.run(
                ["magick", "-size", "32x32", "xc:#d34235", str(source)],
                check=True,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )

            def build(index: int) -> Path:
                target = directory / f"sheet-{index}.png"
                preview.contact_sheet(
                    [(source, f"tile {slot}") for slot in range(6)], target, columns=3
                )
                return target

            with ThreadPoolExecutor(max_workers=3) as executor:
                targets = list(executor.map(build, range(6)))

            self.assertTrue(all(preview.image_has_content(path) for path in targets))
            self.assertFalse(any(path.is_dir() for path in directory.glob(".*-tiles-*")))


class ReviewTests(unittest.TestCase):
    def test_every_profile_resolves_to_a_model_and_valid_transitions(self) -> None:
        models = set()
        for profile in review.PROFILES.values():
            path = preview.model_path(profile.model)
            self.assertTrue(path.is_file())
            self.assertNotIn(profile.model, models)
            models.add(profile.model)
            aspects = preview.aspects_for(profile.model)
            self.assertIn(profile.primary_aspect, aspects)
            for transition in profile.transitions:
                source, target = transition.split(":", 1)
                self.assertIn(source, aspects)
                self.assertIn(target, aspects)
            gltf_paths = [
                dependency
                for dependency in preview.dependencies(path)
                if dependency.suffix == ".gltf"
            ]
            self.assertTrue(gltf_paths)
            node_names = {
                node["name"]
                for gltf in gltf_paths
                for node in json.loads(gltf.read_text(encoding="utf-8"))["nodes"]
            }
            for check in profile.components:
                self.assertIn(check.aspect, aspects)
                self.assertIn(check.view, preview.ALL_VIEWS)
                self.assertTrue(
                    any(node.startswith(check.node) for node in node_names),
                    f"{profile.model}: component prefix {check.node!r} is absent",
                )

    def test_suites_are_deduplicated_and_complete(self) -> None:
        self.assertEqual(set(review.expand_targets(["all"])), set(review.PROFILES))
        for members in review.SUITES.values():
            self.assertEqual(len(members), len(set(members)))
            self.assertTrue(set(members) <= set(review.PROFILES))

    def test_single_review_uses_the_canonical_baseline_filename(self) -> None:
        profile = review.PROFILES["hp-gitter"]
        task = review.single_task(
            "hp-gitter",
            profile,
            Path("/tmp/review-unit"),
            "descriptive label",
            "hp2",
            "rear",
            "head",
            2,
            lod=1,
            background="light",
        )
        self.assertEqual(task.artifact.name, "hp2-rear-head-lod1-bglight.png")

    def test_component_review_names_and_passes_the_target_node(self) -> None:
        profile = review.PROFILES["hp-gitter"]
        task = review.single_task(
            "hp-gitter",
            profile,
            Path("/tmp/review-unit"),
            "upper rear balance",
            "hp0",
            "rear",
            "detail",
            2,
            background="light",
            target_node="gewicht1",
        )
        self.assertEqual(
            task.artifact.name,
            "hp0-rear-detail-nodegewicht1-lod0-bglight.png",
        )
        self.assertIn("--target-node", task.command)
        self.assertIn("gewicht1", task.command)

    def test_review_before_manifest_guards_every_domain_with_explicit_allow_list(self) -> None:
        profile = review.PROFILES["hp-gitter"]
        manifest = Path("/tmp/hp-before.json")
        tasks = review.tasks_for(
            "hp-gitter",
            profile,
            "quick",
            Path("/tmp/review-unit"),
            2,
            (),
            False,
            manifest,
            ("laterne1", "laterne2"),
            "material",
        )
        self.assertTrue(tasks)
        for task in tasks:
            self.assertIn("--protect-component", task.command)
            self.assertEqual(task.command.count("--allow-node"), 2)
            self.assertIn("--allow-domain", task.command)
            self.assertIn("shading", task.command)
            self.assertNotIn("geometry", task.command)
            self.assertIn("laterne1", task.command)
            self.assertIn("laterne2", task.command)


class GeneratorSelectionTests(unittest.TestCase):
    def test_hp_head_hoist_fits_every_height_and_rear_plane(self) -> None:
        for mast in ("gitter", "schmal"):
            layout = gen_form_signals.hp_head_layout(mast)
            for nominal_height in gen_form_signals.HP_PIVOT_HEIGHTS:
                height = gen_form_signals.hp_pivot_above_so(nominal_height)
                centre = gen_form_signals.hp_head_pulley_centre(height, mast)
                cable_runs = gen_form_signals.hp_head_cable_runs(height, mast)

                self.assertEqual(centre[0], layout["pulley_centre_x"])
                if mast == "gitter":
                    self.assertLess(
                        abs(centre[0])
                        + gen_form_signals.HP_HEAD_PULLEY_RADIUS,
                        layout["support_half_width"],
                    )
                else:
                    self.assertAlmostEqual(
                        centre[0]
                        - gen_form_signals.HP_HEAD_PULLEY_RADIUS,
                        layout["support_half_width"],
                    )
                self.assertLess(
                    centre[1] + gen_form_signals.HP_HEAD_PULLEY_RADIUS,
                    height + layout["above_pivot"],
                )
                self.assertEqual(len(cable_runs), 2)
                self.assertLess(cable_runs[0][0][0], centre[0])
                self.assertGreater(cable_runs[1][0][0], centre[0])
                for cable_top, cable_bottom in cable_runs:
                    self.assertEqual(cable_top[0], cable_bottom[0])
                    self.assertEqual(cable_top[1], centre[1])
                    self.assertEqual(
                        cable_bottom[1],
                        gen_form_signals.HP_HEAD_CABLE_BOTTOM,
                    )
                    self.assertEqual(cable_top[2], cable_bottom[2])
                    self.assertEqual(
                        cable_top[2], gen_form_signals.HP_HEAD_CABLE_Z
                    )

                for lod, _distance in gen_form_signals.HP_LODS:
                    components = gen_form_signals.main_static_components(
                        nominal_height, mast, 1, "db_gruen", lod
                    )
                    head = components["mast_head"]
                    max_y = max(
                        y
                        for primitive in head.values()
                        for _x, y, _z in primitive.pos
                    )
                    self.assertLessEqual(
                        max_y,
                        height + layout["above_pivot"] + 1e-9,
                    )

        with self.assertRaises(ValueError):
            gen_form_signals.hp_head_layout("unknown")

    def test_hp_rear_operating_rods_end_in_real_joints(self) -> None:
        for arms in (1, 2):
            drive_joints = gen_form_signals.hp_end_drive_rod_joints(arms)
            self.assertEqual(len(drive_joints), arms)
            for nominal_height in gen_form_signals.HP_PIVOT_HEIGHTS:
                paths = gen_form_signals.hp_operating_rod_paths(
                    nominal_height, arms
                )
                self.assertEqual(len(paths), arms)
                for index, path in enumerate(paths):
                    self.assertEqual(path[0], drive_joints[index])
                    self.assertEqual(len(path), 4)
                    self.assertGreater(path[-1][1], path[-2][1])
                    self.assertLess(abs(path[-1][0]), path[-2][0])

    def test_vr_rods_are_variant_specific_and_end_in_real_joints(self) -> None:
        for centre_y in gen_form_signals.VR_CENTRE_HEIGHTS:
            for mast_style in ("1944", "alt_u"):
                mast_width, mast_depth = gen_form_signals.vr_mast_envelope(
                    mast_style
                )
                expected_x = (
                    mast_width * 0.5
                    + gen_form_signals.VR_OPERATING_ROD_MAST_CLEARANCE
                )
                expected_z = (
                    -mast_depth * 0.5
                    - gen_form_signals.VR_OPERATING_ROD_REAR_OFFSET
                )
                for aspects in (2, 3):
                    for drive in ("drahtzug", "elektro"):
                        paths = gen_form_signals.vr_operating_rod_paths(
                            centre_y, aspects, drive, mast_style
                        )
                        upper = gen_form_signals.vr_upper_operating_joints(
                            centre_y, aspects
                        )
                        self.assertEqual(len(paths), aspects - 1)
                        self.assertEqual(len(upper), aspects - 1)
                        for index, path in enumerate(paths):
                            self.assertEqual(len(path), 5)
                            self.assertEqual(
                                path[0],
                                gen_form_signals.vr_drive_output_joint(
                                    drive, aspects
                                ),
                            )
                            self.assertEqual(path[-1], upper[index])
                            self.assertAlmostEqual(
                                abs(path[2][0]), expected_x
                            )
                            self.assertEqual(path[2][2], expected_z)
                            self.assertEqual(path[3][2], expected_z)
                            self.assertGreater(path[3][1], path[2][1])
                            # The discarded decorative pair sat at 355/425 mm
                            # and ended in open air beside the lanterns.
                            self.assertNotIn(abs(path[2][0]), (0.355, 0.425))

    def test_vr_disc_crank_belongs_only_to_the_moving_disc(self) -> None:
        """Vr 1 must not leave a duplicate fixed crank standing in free air."""
        for lod in (0, 1):
            moving = gen_form_signals.vr_disc_mesh(lod)[
                gen_form_signals.MAT_BLACK
            ]
            pin = gen_form_signals.VR_DISC_CRANK_PIN
            self.assertLess(
                min(
                    sum((coordinate - target) ** 2 for coordinate, target in zip(vertex, pin))
                    for vertex in moving.pos
                ) ** 0.5,
                0.035,
            )

            for centre_y in gen_form_signals.VR_CENTRE_HEIGHTS:
                fixed = gen_form_signals.distant_static_prims(
                    centre_y,
                    2,
                    "db_gruen",
                    "am_mast",
                    "led",
                    "drahtzug",
                    "1944",
                    lod,
                )["dark"]
                global_pin = (
                    pin[0],
                    centre_y + pin[1],
                    0.31 + pin[2],
                )
                self.assertGreater(
                    min(
                        sum(
                            (coordinate - target) ** 2
                            for coordinate, target in zip(vertex, global_pin)
                        )
                        for vertex in fixed.pos
                    ) ** 0.5,
                    0.040,
                )

    def test_vr_electric_leads_join_lamps_box_and_mast_for_both_masts(self) -> None:
        for centre_y in gen_form_signals.VR_CENTRE_HEIGHTS:
            layout = gen_form_signals.vr_night_layout(centre_y)
            fixed_axes = (layout["left_amber"], layout["right_amber"])
            for mast_style in ("1944", "alt_u"):
                mast_width, mast_depth = gen_form_signals.vr_mast_envelope(
                    mast_style
                )
                wiring = gen_form_signals.vr_electric_lighting_paths(
                    centre_y, mast_style
                )
                self.assertEqual(len(wiring["branches"]), 2)
                junction = wiring["junction"]
                junction_top_y = (
                    junction[1] + wiring["junction_half_height"]
                )
                for axis, path in zip(fixed_axes, wiring["branches"]):
                    self.assertEqual(path[0][0], axis[0])
                    self.assertAlmostEqual(path[0][1], axis[1] - 0.145)
                    self.assertEqual(path[-1][1], junction_top_y)
                    self.assertEqual(path[-1][2], junction[2])
                    self.assertLessEqual(abs(path[-2][0]), mast_width * 0.5)
                    for start, end in zip(path, path[1:]):
                        self.assertNotEqual(start, end)

                trunk = wiring["trunk"]
                self.assertEqual(
                    trunk[0],
                    (
                        junction[0],
                        junction[1] - wiring["junction_half_height"],
                        junction[2],
                    ),
                )
                self.assertEqual(trunk[-1][0:2], (0.0, 0.240))
                self.assertEqual(trunk[-1][2], -mast_depth * 0.5)
                self.assertEqual(wiring["mast_rear_z"], -mast_depth * 0.5)
                self.assertLess(wiring["conduit_z"], wiring["mast_rear_z"])

    def test_hp_lantern_back_clears_mast_and_leads_reach_terminal(self) -> None:
        # The 151-mm outer half-width includes the folded side cheek.  It must
        # clear even the 145-mm Gittermast chord plus its 15-mm half profile.
        inner_lantern_edge = gen_form_signals.HP_SELECTOR_RADIUS - 0.151
        self.assertGreater(inner_lantern_edge, 0.145 + 0.015)
        self.assertGreater(
            gen_form_signals.HP_RETURN_SPRING_X, 0.0,
            "return spring belongs between the outboard lantern and mast",
        )

        lamp_x, lamp_dy = gen_form_signals.hp_lamp_offset(False)
        for nominal_height in gen_form_signals.HP_PIVOT_HEIGHTS:
            upper_y, _lower_y = gen_form_signals.hp_pivot_levels(
                nominal_height
            )
            paths = gen_form_signals.hp_lantern_lead_paths(nominal_height)
            self.assertEqual(len(paths), 2)
            for side, path in zip((-1.0, 1.0), paths):
                lead_top, lead_knee, lead_bottom = path
                expected_x = lamp_x + side * (
                    gen_form_signals.HP_LANTERN_LEAD_GLAND_OFFSET
                )
                self.assertEqual(lead_top[0], expected_x)
                self.assertEqual(lead_knee[0], expected_x)
                self.assertAlmostEqual(lead_top[1], upper_y + lamp_dy - 0.052)
                self.assertEqual(
                    lead_knee[1], gen_form_signals.HP_LANTERN_LEAD_KNEE_Y
                )
                self.assertEqual(
                    lead_bottom[1], gen_form_signals.HP_LANTERN_LEAD_BOTTOM
                )
                self.assertEqual(
                    lead_bottom[0],
                    gen_form_signals.HP_LANTERN_TERMINAL_X
                    + side
                    * gen_form_signals.HP_LANTERN_TERMINAL_HALF_SPACING,
                )
                self.assertEqual(
                    lead_top[2], gen_form_signals.HP_LANTERN_LEAD_Z
                )
                self.assertTrue(all(point[2] == lead_top[2] for point in path))

    def test_hp_white_field_keeps_the_red_end_ring_closed(self) -> None:
        for lower in (False, True):
            for shortened in (False, True):
                expected = gen_form_signals.hp_arm_stripe_end(
                    lower, shortened
                )
                for lod, _distance in gen_form_signals.HP_LODS:
                    blade = gen_form_signals.main_arm_mesh(
                        lower, shortened, False, lod
                    )
                    # main_arm_mesh emits the straight front enamel box first.
                    # Its end must not enter the circular red annulus at any LOD.
                    front_box = blade[gen_form_signals.MAT_WHITE].pos[:36]
                    longitudinal = [
                        y if lower else x for x, y, _z in front_box
                    ]
                    self.assertAlmostEqual(max(longitudinal), expected)

                length, _width, diameter = gen_form_signals.HP_ARM[
                    (lower, shortened)
                ]
                disc_radius = diameter * 0.5
                disc_centre = (
                    length - gen_form_signals.HP_ARM_ROOT[lower] - disc_radius
                )
                self.assertAlmostEqual(expected, disc_centre - disc_radius)

    def test_hp_rear_equalisers_are_separate_and_bound_per_blade(self) -> None:
        ron = gen_form_signals.main_model_ron(
            "test.gltf", 2, ["test"], coupled=False
        )
        for level, _distance in gen_form_signals.HP_LODS:
            self.assertIn(f'node: "gewicht_ausgleich1_LOD{level}"', ron)
            self.assertIn(f'node: "gewicht_ausgleich2_LOD{level}"', ron)
        self.assertIn(
            f"degrees: {-gen_form_signals.HP_EQUALIZER_SWING_DEGREES:.1f}",
            ron,
        )
        self.assertIn(
            f"degrees: {gen_form_signals.HP_EQUALIZER_SWING_DEGREES:.1f}",
            ron,
        )

    def test_fast_generator_catalogue_matches_full_geometry_and_coupled_counts(self) -> None:
        specs = gen_form_signals.catalogue_model_specs()
        coupled = [name for name in specs if name.endswith("_gekuppelt")]
        geometry = [name for name in specs if not name.endswith("_gekuppelt")]
        self.assertEqual(len(geometry), 188)
        self.assertEqual(len(coupled), 42)
        self.assertEqual(specs["form_hp_8m_gitter_2fl"]["kind"], "hp")
        self.assertEqual(specs["form_vr_4_87m_3begr_gas"]["lighting"], "gas")
        self.assertEqual(specs["form_sh_hoch"]["high"], True)
        self.assertTrue(
            {profile.model for profile in review.PROFILES.values()} <= set(specs)
        )

    def test_hp_fixed_assemblies_are_independent_lod_nodes(self) -> None:
        asset = ROOT / "mods/example/assets/sig_form_hp_8m_gitter_2fl.gltf"
        nodes = {
            node["name"]
            for node in json.loads(asset.read_text(encoding="utf-8"))["nodes"]
        }
        for stem in gen_form_signals.HP_STATIC_STEMS:
            for level, _distance in gen_form_signals.HP_LODS:
                self.assertIn(f"{stem}_LOD{level}", nodes)
        self.assertNotIn("mast_LOD0", nodes)
        self.assertNotIn(
            "mast_board", gen_form_signals.hp_static_stems("altanstrich")
        )

    def test_hp_gitter_opposing_faces_are_mirrored(self) -> None:
        for bay in range(8):
            front = gen_form_signals.hp_gitter_brace_direction(bay, 1)
            rear = gen_form_signals.hp_gitter_brace_direction(bay, -1)
            self.assertEqual(front, -rear)
            if bay:
                previous = gen_form_signals.hp_gitter_brace_direction(bay - 1, 1)
                self.assertEqual(front, -previous)
        with self.assertRaises(ValueError):
            gen_form_signals.hp_gitter_brace_direction(0, 0)


class WorkbenchTests(unittest.TestCase):
    def test_capture_plan_contains_context_attachment_isolation_and_motion(self) -> None:
        captures = workbench.capture_plan(
            review.PROFILES["hp-gitter"], "fluegel1"
        )
        self.assertEqual(len(captures), len({capture.slug for capture in captures}))
        by_slug = {capture.slug: capture for capture in captures}
        self.assertIn("overall-front", by_slug)
        self.assertEqual(by_slug["component-attached"].target_node, "fluegel1")
        self.assertFalse(by_slug["component-attached"].isolate_target)
        self.assertTrue(by_slug["component-isolated"].isolate_target)
        self.assertEqual(by_slug["motion"].transition, "hp2:hp0")

    def test_equaliser_motion_capture_follows_mid_mast_component(self) -> None:
        captures = workbench.capture_plan(
            review.PROFILES["hp-schmal"],
            "gewicht_ausgleich1",
            aspect="hp0",
            view="rear",
            background="light",
        )
        motion = next(capture for capture in captures if capture.slug == "motion")
        self.assertEqual(motion.view, "rear")
        self.assertEqual(motion.focus, "detail")
        self.assertEqual(motion.background, "light")
        self.assertEqual(motion.target_node, "gewicht_ausgleich1")

    def test_dimension_delta_is_reported_in_centimetres(self) -> None:
        before = {"framed_cm": [100.0, 200.0, 30.0]}
        after = {"framed_cm": [101.25, 199.5, 30.0]}
        self.assertEqual(
            workbench.dimension_delta(before, after), [1.25, -0.5, 0.0]
        )

    def test_resume_preserves_baseline_and_continues_iteration_numbers(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            profile = review.PROFILES["hp-gitter"]
            captures = workbench.capture_plan(profile, "fluegel1", "hp0")
            guard = root / "baseline/context-front.json"
            guard.parent.mkdir()
            guard.write_text("{}\n", encoding="utf-8")
            (root / "iterations/0003").mkdir(parents=True)
            config = {
                "profile": "hp-gitter",
                "model": profile.model,
                "component": "fluegel1",
                "kind": "geometry",
                "allowed_nodes": ["fluegel1"],
                "allowed_domains": ["geometry"],
                "guard_manifest": "baseline/context-front.json",
                "captures": [workbench.asdict(capture) for capture in captures],
            }
            (root / "session.json").write_text(
                json.dumps(config), encoding="utf-8"
            )
            (root / "state.json").write_text(
                json.dumps({"current_iteration": 2, "logs": ["kept"]}),
                encoding="utf-8",
            )

            restored = workbench.SignalWorkbench.__new__(workbench.SignalWorkbench)
            restored.root = root
            restored.profile_name = "hp-gitter"
            restored.profile = profile
            restored.component = "fluegel1"
            restored.kind = "geometry"
            restored.allowed_nodes = ("fluegel1",)
            restored.allowed_domains = ("geometry",)
            restored.captures = captures
            restored.lock = threading.Lock()
            restored.baseline = []
            restored.iteration = 0
            evidence = [{"slug": "fixed-before"}]
            restored.read_pack = lambda _directory: evidence

            restored.resume_session()

            self.assertIs(restored.baseline, evidence)
            self.assertEqual(restored.iteration, 3)
            self.assertEqual(restored.guard_manifest, guard)
            self.assertEqual(restored.state["phase"], "ready")
            self.assertEqual(restored.state["logs"], ["kept"])
            self.assertIn("nächste Iteration 4", restored.state["message"])

    def test_workbench_evidence_defaults_to_ignored_target_tree(self) -> None:
        self.assertEqual(
            workbench.DEFAULT_OUTPUT, ROOT / "target/signal-workbench"
        )


if __name__ == "__main__":
    unittest.main()
