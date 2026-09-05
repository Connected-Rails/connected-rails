#!/usr/bin/env python3
"""Build a repeatable review pack for one canonical form-signal construction.

The small ``preview.py`` command is the fast microscope for one screenshot.
This wrapper is the safety gate: it regenerates the catalogue, builds the real
Bevy preview renderer once and then records the views, states, LODs and motion
samples that are easy to forget during an iteration.

Examples:
  python tools/signals/review.py hp-gitter --mode quick
  python tools/signals/review.py vr-electric --mode standard --reference photo.jpg
  python tools/signals/review.py core --mode full
  python tools/signals/review.py --list
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
import hashlib
import html
import json
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[2]
PREVIEW = ROOT / "tools/signals/preview.py"
DEFAULT_OUTPUT = Path("/tmp/connected-rails-signal-review")
QUICK_MOTION_TIMES = "0,0.55,0.78,1.02,1.80"
EDIT_DOMAINS = {
    "geometry": ("geometry",),
    "material": ("shading",),
    "animation": ("binding",),
    "assembly": ("geometry", "shading", "binding"),
}


@dataclass(frozen=True)
class ComponentCheck:
    node: str
    aspect: str
    view: str
    label: str
    background: str = "neutral"


@dataclass(frozen=True)
class Profile:
    model: str
    primary_aspect: str
    transitions: tuple[str, ...] = ()
    purpose: str = ""
    components: tuple[ComponentCheck, ...] = ()


HP_FIXED_COMPONENTS = (
    ComponentCheck("mast_structure", "hp0", "front", "mast lattice / front"),
    ComponentCheck("mast_structure", "hp0", "right", "mast lattice / side", "light"),
    ComponentCheck("mast_board", "hp0", "front", "recognition boards / front"),
    ComponentCheck("mast_board", "hp0", "rear", "recognition boards / rear", "light"),
    ComponentCheck("mast_head", "hp0", "rear", "mast head and pulley / rear", "light"),
    ComponentCheck("mast_rods", "hp0", "rear", "operating rods / rear", "light"),
    ComponentCheck("mast_drive", "hp0", "rear", "end drive / rear", "light"),
)
HP_MOVING_COMPONENTS = (
    ComponentCheck("fluegel1", "hp2", "front", "upper blade / front"),
    ComponentCheck("fluegel2", "hp0", "front", "lower blade parked / front"),
    ComponentCheck("gewicht1", "hp0", "rear", "upper balance / rear", "light"),
    ComponentCheck("gewicht2", "hp0", "rear", "lower blade holder / rear", "light"),
    ComponentCheck("gewicht_ausgleich1", "hp0", "rear",
                   "mid-mast equalising lever / rear", "light"),
    ComponentCheck("gewicht_ausgleich2", "hp0", "rear",
                   "second mid-mast equalising lever / rear", "light"),
    ComponentCheck("blende1", "hp2", "front", "upper colour selector / front"),
    ComponentCheck("laterne1", "hp0", "rear", "upper lamp body / rear", "light"),
)
HP_COMPONENTS = HP_FIXED_COMPONENTS + HP_MOVING_COMPONENTS
HP_HISTORIC_COMPONENTS = tuple(
    check for check in HP_COMPONENTS if check.node != "mast_board"
)
VR_THREE_COMPONENTS = (
    ComponentCheck("scheibe", "vr0", "front", "disc face / front"),
    ComponentCheck("scheibe", "vr1", "right", "folded disc / side", "light"),
    ComponentCheck("vorsignalfluegel", "vr2", "front", "additional wing / front"),
    ComponentCheck("farbblende_right", "vr2", "front-right", "colour filter / quarter"),
    ComponentCheck("antrieb", "vr2", "right", "drive / side", "light"),
)
VR_TWO_COMPONENTS = (
    ComponentCheck("scheibe", "vr0", "front", "disc face / front"),
    ComponentCheck("scheibe", "vr1", "right", "folded disc / side", "light"),
    ComponentCheck("farbblende_right", "vr1", "front-right", "colour filter / quarter"),
    ComponentCheck("antrieb", "vr1", "right", "drive / side", "light"),
)
SH_COMPONENTS = (
    ComponentCheck("sperrscheibe", "sh0", "front", "bar at Sh 0 / front"),
    ComponentCheck("sperrscheibe", "sh1", "rear", "bar at Sh 1 / rear", "light"),
)


# One representative per genuinely different construction.  Pure height and
# paint variants are deliberately separate profiles: they are cheap to call
# when those shared parameters change, but do not slow every blade edit.
PROFILES: dict[str, Profile] = {
    "hp-gitter": Profile(
        "form_hp_8m_gitter_2fl", "hp2", ("hp2:hp0", "hp1:hp0"),
        "long two-arm Hauptsignal on the lattice construction",
        HP_COMPONENTS,
    ),
    "hp-schmal": Profile(
        "form_hp_8m_schmal_2fl", "hp2", ("hp2:hp0", "hp1:hp0"),
        "long two-arm Hauptsignal on the narrow welded mast",
        HP_COMPONENTS,
    ),
    "hp-short": Profile(
        "form_hp_8m_gitter_2fl_kurz", "hp2", ("hp2:hp0", "hp1:hp0"),
        "short upper and lower blade outlines",
        HP_COMPONENTS,
    ),
    "hp-coupled": Profile(
        "form_hp_8m_gitter_2fl_gekuppelt", "hp2", ("hp2:hp0",),
        "mechanically coupled Hp0/Hp2 drive",
        HP_COMPONENTS,
    ),
    "hp-grey": Profile(
        "form_hp_8m_gitter_2fl_eisengrau", "hp2",
        purpose="iron-grey paint and material response",
        components=HP_COMPONENTS,
    ),
    "hp-historic-paint": Profile(
        "form_hp_8m_gitter_2fl_altanstrich", "hp2",
        purpose="historic red/white/black mast paint",
        components=HP_HISTORIC_COMPONENTS,
    ),
    "vr-electric": Profile(
        "form_vr_4_87m_3begr", "vr2", ("vr1:vr0", "vr0:vr2", "vr2:vr0"),
        "three-aspect wire-driven Vorsignal with electric night sign",
        VR_THREE_COMPONENTS,
    ),
    "vr-electric-drive": Profile(
        "form_vr_4_87m_3begr_elektroantrieb", "vr2",
        ("vr1:vr0", "vr0:vr2", "vr2:vr0"),
        "three-aspect Vorsignal with Siemens motor drive and release attachment",
        VR_THREE_COMPONENTS,
    ),
    "vr-old-u-two": Profile(
        "form_vr_4_87m_2begr_altmast", "vr1", ("vr1:vr0",),
        "old two-channel mast, two-aspect wire-driven conversion",
        VR_TWO_COMPONENTS,
    ),
    "vr-old-u-three": Profile(
        "form_vr_4_87m_3begr_elektroantrieb_altmast", "vr2",
        ("vr1:vr0", "vr0:vr2", "vr2:vr0"),
        "old two-channel mast, three-aspect Siemens-driven construction",
        VR_THREE_COMPONENTS,
    ),
    "vr-gas": Profile(
        "form_vr_4_87m_3begr_gas", "vr2", ("vr1:vr0", "vr0:vr2", "vr2:vr0"),
        "three-aspect gas Vorsignal with bottle and open cage",
        VR_THREE_COMPONENTS + (
            ComponentCheck("gas_cartridges", "vr2", "rear-right", "gas bottle / rear"),
        ),
    ),
    "vr-two-aspect": Profile(
        "form_vr_4_87m_2begr", "vr1", ("vr1:vr0",),
        "two-aspect Vorsignal without the lower wing",
        VR_TWO_COMPONENTS,
    ),
    "vr-free-ne2": Profile(
        "form_vr_4_87m_3begr_ohne_ne2", "vr2", ("vr1:vr0", "vr0:vr2"),
        "Vorsignal prepared for a freestanding Ne 2 board",
        VR_THREE_COMPONENTS,
    ),
    "vr-low": Profile(
        "form_vr_2_76m_3begr", "vr2",
        purpose="2.76 m disc-centre installation height",
        components=VR_THREE_COMPONENTS,
    ),
    "vr-high": Profile(
        "form_vr_5_37m_3begr", "vr2",
        purpose="5.37 m disc-centre installation height",
        components=VR_THREE_COMPONENTS,
    ),
    "vr-grey": Profile(
        "form_vr_4_87m_3begr_eisengrau", "vr2",
        purpose="iron-grey Vorsignal paint and material response",
        components=VR_THREE_COMPONENTS,
    ),
    "sh-high": Profile(
        "form_sh_hoch", "sh0", ("sh1:sh0", "sh0:sh1"),
        "high Form-Sperrsignal",
        SH_COMPONENTS,
    ),
    "sh-low": Profile(
        "form_sh_niedrig", "sh0", ("sh1:sh0", "sh0:sh1"),
        "low Form-Sperrsignal",
        SH_COMPONENTS,
    ),
    "sh-grey": Profile(
        "form_sh_hoch_eisengrau", "sh0",
        purpose="iron-grey Sperrsignal paint and material response",
        components=SH_COMPONENTS,
    ),
    "ne2-high": Profile(
        "form_ne2_frei_hoch", "none", purpose="freestanding 750 x 480 mm Ne 2",
        components=(ComponentCheck("tafel", "none", "front", "Ne 2 face / front"),),
    ),
    "ne2-low": Profile(
        "form_ne2_frei_niedrig", "none", purpose="freestanding 450 x 300 mm Ne 2",
        components=(ComponentCheck("tafel", "none", "front", "Ne 2 face / front"),),
    ),
}

SUITES: dict[str, tuple[str, ...]] = {
    "core": ("hp-gitter", "hp-schmal", "vr-electric",
             "vr-electric-drive", "vr-old-u-three", "vr-gas",
             "sh-high", "sh-low"),
    "hp": tuple(name for name in PROFILES if name.startswith("hp-")),
    "vr": tuple(name for name in PROFILES if name.startswith("vr-"))
    + ("ne2-high", "ne2-low"),
    "sh": tuple(name for name in PROFILES if name.startswith("sh-")),
    "all": tuple(PROFILES),
}


@dataclass(frozen=True)
class Task:
    profile: str
    label: str
    command: tuple[str, ...]
    artifact: Path


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def run(command: list[str] | tuple[str, ...]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        list(command), cwd=ROOT, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE
    )


def checked(command: list[str]) -> None:
    result = run(command)
    if result.stdout:
        print(result.stdout, end="")
    if result.returncode:
        if result.stderr:
            print(result.stderr, file=sys.stderr, end="")
        raise SystemExit(result.returncode)


def expand_targets(values: list[str]) -> list[str]:
    expanded: list[str] = []
    for value in values:
        names = SUITES.get(value, (value,))
        for name in names:
            if name not in PROFILES:
                valid = ", ".join(sorted((*PROFILES, *SUITES)))
                raise SystemExit(f"unknown review target {value!r}; choose one of: {valid}")
            if name not in expanded:
                expanded.append(name)
    return expanded


def preview_base(profile: Profile, frames: int) -> list[str]:
    return [
        sys.executable,
        str(PREVIEW),
        profile.model,
        "--no-build",
        "--frames",
        str(frames),
    ]


def single_task(
    name: str,
    profile: Profile,
    run_dir: Path,
    label: str,
    aspect: str,
    view: str,
    focus: str,
    frames: int,
    *,
    lod: int = 0,
    background: str = "neutral",
    target_node: str | None = None,
    references: tuple[Path, ...] = (),
) -> Task:
    # Keep preview.py's canonical basename even inside the review pack.  A
    # baseline approved from a one-off render must also guard this same view.
    node_tag = f"-node{target_node}" if target_node else ""
    target = run_dir / name / (
        f"{aspect}-{view}-{focus}{node_tag}-lod{lod}-bg{background}.png"
    )
    command = preview_base(profile, frames) + [
        "--aspect", aspect,
        "--view", view,
        "--focus", focus,
        "--lod", str(lod),
        "--background", background,
        "--output", str(target),
    ]
    if target_node:
        command += ["--target-node", target_node]
    for reference in references:
        command += ["--reference", str(reference)]
    artifact = (
        target.with_name(f"{target.stem}-contact-sheet.png") if references else target
    )
    return Task(name, label, tuple(command), artifact)


def tasks_for(
    name: str,
    profile: Profile,
    mode: str,
    run_dir: Path,
    frames: int,
    references: tuple[Path, ...],
    compare_baseline: bool,
    before_manifest: Path | None = None,
    changed_nodes: tuple[str, ...] = (),
    edit_kind: str = "assembly",
) -> list[Task]:
    base = preview_base(profile, frames)
    target = run_dir / name
    tasks: list[Task] = []

    if mode == "quick":
        for view, background in (("front", "neutral"), ("left", "neutral"), ("rear", "light")):
            tasks.append(single_task(
                name, profile, run_dir, f"{profile.primary_aspect}-{view}-head",
                profile.primary_aspect, view, "head", frames,
                background=background,
                references=references if view == "front" else (),
            ))
        if not profile.components:
            tasks.append(single_task(
                name, profile, run_dir,
                f"{profile.primary_aspect}-front-left-detail",
                profile.primary_aspect, "front-left", "detail", frames,
            ))
    else:
        matrix_dir = target / "states-head"
        command = base + [
            "--matrix", "--focus", "head", "--background", "neutral",
            "--output", str(matrix_dir),
        ]
        for reference in references:
            command += ["--reference", str(reference)]
        tasks.append(Task(name, "all states / six views / head", tuple(command),
                          matrix_dir / "contact-sheet.png"))
        tasks.append(single_task(
            name, profile, run_dir, f"{profile.primary_aspect}-front-full",
            profile.primary_aspect, "front", "full", frames,
        ))
        tasks.append(single_task(
            name, profile, run_dir, f"{profile.primary_aspect}-rear-full",
            profile.primary_aspect, "rear", "full", frames, background="light",
        ))

    # Explicit component microscopes prevent a correct blade, disc or drive
    # from being hidden by an otherwise plausible whole-head shot. They use
    # node-prefix framing in the real renderer, not a post-render pixel crop.
    for check in profile.components:
        tasks.append(single_task(
            name,
            profile,
            run_dir,
            f"component: {check.label}",
            check.aspect,
            check.view,
            "detail",
            frames,
            background=check.background,
            target_node=check.node,
        ))

    transitions = profile.transitions
    if mode == "quick":
        # Two compact strips are enough to expose both independent Vr
        # mechanisms. Other profiles need only their first critical fall.
        transitions = transitions[:2] if name.startswith("vr-") else transitions[:1]
    for transition in transitions:
        source, destination = transition.split(":", 1)
        motion_dir = target / f"motion-{source}-to-{destination}"
        command = base + [
            "--animation", transition,
            "--view", "front-left",
            "--focus", "head",
            "--output", str(motion_dir),
        ]
        if mode == "quick":
            command += ["--animation-times", QUICK_MOTION_TIMES]
        tasks.append(Task(name, f"motion {source} → {destination}", tuple(command),
                          motion_dir / "animation-strip.png"))

    if mode == "full":
        full_dir = target / "states-full"
        tasks.append(Task(
            name,
            "all states / six views / full height",
            tuple(base + [
                "--matrix", "--focus", "full", "--background", "neutral",
                "--output", str(full_dir),
            ]),
            full_dir / "contact-sheet.png",
        ))
        for lod in (1, 2):
            for view, background in (("front", "neutral"), ("rear", "light")):
                tasks.append(single_task(
                    name, profile, run_dir,
                    f"{profile.primary_aspect}-{view}-head-lod{lod}",
                    profile.primary_aspect, view, "head", frames,
                    lod=lod, background=background,
                ))
        for view, background in (("front", "neutral"), ("rear", "light")):
            tasks.append(single_task(
                name, profile, run_dir, f"{profile.primary_aspect}-{view}-base",
                profile.primary_aspect, view, "base", frames, background=background,
            ))
        if not profile.components:
            tasks.append(single_task(
                name, profile, run_dir,
                f"{profile.primary_aspect}-front-left-detail",
                profile.primary_aspect, "front-left", "detail", frames,
            ))

    if before_manifest is not None:
        guard_args = ["--protect-component", str(before_manifest)]
        for prefix in changed_nodes:
            guard_args += ["--allow-node", prefix]
        for domain in EDIT_DOMAINS[edit_kind]:
            guard_args += ["--allow-domain", domain]
        tasks = [Task(task.profile, task.label,
                      (*task.command, *guard_args), task.artifact)
                 for task in tasks]
    if compare_baseline:
        tasks = [Task(task.profile, task.label,
                      (*task.command, "--compare-baseline"), task.artifact)
                 for task in tasks]
    return tasks


def write_report(
    run_dir: Path,
    mode: str,
    names: list[str],
    tasks: list[Task],
    references: tuple[Path, ...],
    before_manifest: Path | None,
    changed_nodes: tuple[str, ...],
    edit_kind: str,
) -> None:
    cards = []
    for name in names:
        profile = PROFILES[name]
        cards.append(
            f"<section><h2>{html.escape(name)}</h2>"
            f"<p><code>{html.escape(profile.model)}</code> — {html.escape(profile.purpose)}</p>"
            "<div class=grid>"
        )
        for task in (task for task in tasks if task.profile == name):
            relative = task.artifact.relative_to(run_dir)
            cards.append(
                "<figure>"
                f"<a href=\"{html.escape(relative.as_posix())}\">"
                f"<img loading=lazy src=\"{html.escape(relative.as_posix())}\"></a>"
                f"<figcaption>{html.escape(task.label)}</figcaption>"
                "</figure>"
            )
        cards.append("</div></section>")

    document = f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Form-signal review</title>
<style>
body{{margin:24px;background:#15171a;color:#eee;font:15px system-ui,sans-serif}}
h1,h2{{font-weight:650}} code{{color:#b8d7ff}}
.grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:18px}}
figure{{margin:0;padding:10px;background:#25282d;border-radius:7px}}
img{{display:block;width:100%;max-height:620px;object-fit:contain;background:#111}}
figcaption{{padding:9px 2px 2px;color:#d8d8d8}}
</style></head><body>
<h1>Form-signal review · {html.escape(mode)}</h1>
<p>Generated {datetime.now(timezone.utc).isoformat()} · references: {len(references)} ·
protected before-manifest: {html.escape(str(before_manifest) if before_manifest else "none")}</p>
<p>edit kind: {html.escape(edit_kind)} · allowed domains:
{html.escape(', '.join(EDIT_DOMAINS[edit_kind]))}</p>
{''.join(cards)}
</body></html>"""
    (run_dir / "index.html").write_text(document, encoding="utf-8")

    manifest = {
        "created_at": datetime.now(timezone.utc).isoformat(),
        "mode": mode,
        "profiles": {name: asdict(PROFILES[name]) for name in names},
        "references": [
            {"path": str(path), "sha256": file_sha256(path)} for path in references
        ],
        "before_manifest": str(before_manifest) if before_manifest else None,
        "allowed_changed_node_prefixes": list(changed_nodes),
        "edit_kind": edit_kind,
        "allowed_domains": list(EDIT_DOMAINS[edit_kind]),
        "generator_sha256": file_sha256(ROOT / "tools/gen_form_signals.py"),
        "preview_sha256": file_sha256(PREVIEW),
        "tasks": [
            {
                "profile": task.profile,
                "label": task.label,
                "command": list(task.command),
                "artifact": str(task.artifact.relative_to(run_dir)),
            }
            for task in tasks
        ],
    }
    (run_dir / "review.json").write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("targets", nargs="*", help="profile or suite name")
    parser.add_argument("--list", action="store_true", help="list profiles and suites")
    parser.add_argument("--mode", choices=("quick", "standard", "full"), default="standard")
    parser.add_argument("--reference", type=Path, action="append", default=[])
    parser.add_argument("--output", type=Path)
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--frames", type=int, default=35)
    parser.add_argument("--no-generate", action="store_true")
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument("--compare-baseline", action="store_true")
    parser.add_argument(
        "--before-manifest",
        type=Path,
        help="per-node geometry guard manifest captured before this component edit",
    )
    parser.add_argument(
        "--changed-node",
        action="append",
        default=[],
        metavar="PREFIX",
        help="node prefix intentionally changed since --before-manifest (repeatable)",
    )
    parser.add_argument(
        "--edit-kind",
        choices=tuple(EDIT_DOMAINS),
        default="assembly",
        help=(
            "domain allowed on --changed-node: geometry, material, animation, "
            "or all three for assembly (default)"
        ),
    )
    args = parser.parse_args()

    if args.list:
        print("Profiles:")
        for name, profile in PROFILES.items():
            print(f"  {name:20} {profile.model:45} {profile.purpose}")
        print("Suites:")
        for name, members in SUITES.items():
            print(f"  {name:20} {', '.join(members)}")
        return
    if not args.targets:
        parser.error("supply a profile/suite or use --list")
    if not 1 <= args.jobs <= 4:
        parser.error("--jobs must be between 1 and 4")
    if args.frames < 1:
        parser.error("--frames must be positive")
    references = tuple(path.resolve() for path in args.reference)
    missing = [path for path in references if not path.is_file()]
    if missing:
        parser.error("missing reference image(s): " + ", ".join(map(str, missing)))

    names = expand_targets(args.targets)
    before_manifest = args.before_manifest.resolve() if args.before_manifest else None
    changed_nodes = tuple(args.changed_node)
    if before_manifest is not None and not before_manifest.is_file():
        parser.error(f"missing before-manifest: {before_manifest}")
    if before_manifest is not None and not changed_nodes:
        parser.error("--before-manifest requires at least one --changed-node")
    if changed_nodes and before_manifest is None:
        parser.error("--changed-node requires --before-manifest")
    if args.edit_kind != "assembly" and before_manifest is None:
        parser.error("--edit-kind needs --before-manifest")
    if before_manifest is not None and len(names) != 1:
        parser.error("a before-manifest can guard exactly one review profile")
    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    run_dir = (args.output or DEFAULT_OUTPUT / f"{stamp}-{os.getpid()}").resolve()
    try:
        run_dir.mkdir(parents=True, exist_ok=False)
    except FileExistsError:
        parser.error(f"output already exists; choose a new directory: {run_dir}")

    if not args.no_generate:
        checked([sys.executable, "tools/gen_form_signals.py"])
    if not args.no_build:
        checked(["cargo", "build", "-p", "signal-editor"])

    binary = ROOT / "target/debug/trainsim-signal-editor"
    if not binary.is_file():
        raise SystemExit(f"missing preview renderer {binary}; omit --no-build once")

    tasks: list[Task] = []
    for name in names:
        tasks.extend(tasks_for(
            name, PROFILES[name], args.mode, run_dir, args.frames,
            references, args.compare_baseline, before_manifest, changed_nodes,
            args.edit_kind,
        ))

    failures: list[tuple[Task, subprocess.CompletedProcess[str]]] = []
    with ThreadPoolExecutor(max_workers=args.jobs) as executor:
        pending = {executor.submit(run, task.command): task for task in tasks}
        for future in as_completed(pending):
            task = pending[future]
            result = future.result()
            if result.returncode:
                failures.append((task, result))
                print(f"FAIL {task.profile}: {task.label}", file=sys.stderr)
            else:
                print(f"OK   {task.profile}: {task.label}")

    if failures:
        for task, result in failures:
            print("COMMAND " + " ".join(task.command), file=sys.stderr)
            if result.stdout:
                print(result.stdout, file=sys.stderr, end="")
            if result.stderr:
                print(result.stderr, file=sys.stderr, end="")
        raise SystemExit(f"{len(failures)} review task(s) failed; partial output: {run_dir}")

    missing_artifacts = [task.artifact for task in tasks if not task.artifact.is_file()]
    if missing_artifacts:
        raise SystemExit(
            "review command succeeded without artifact(s): "
            + ", ".join(map(str, missing_artifacts))
        )
    write_report(
        run_dir, args.mode, names, tasks, references,
        before_manifest, changed_nodes, args.edit_kind,
    )
    print(run_dir / "index.html")


if __name__ == "__main__":
    main()
