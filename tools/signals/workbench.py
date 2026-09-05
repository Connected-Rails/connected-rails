#!/usr/bin/env python3
"""Live, component-scoped workbench for German form-signal iteration.

The workbench records an immutable before pack with the simulator's real Bevy
renderer, serves it in a browser and watches the procedural source.  Every
later edit is regenerated for one canonical model, checked *before rendering*
against the declared component and edit kind, then shown as before/current/diff
views.  Unrelated geometry, PBR response and animation bindings are blocked.

Example:
  python tools/signals/workbench.py hp-gitter --component fluegel1 \
    --kind geometry --reference /path/to/prototype.jpg --port 4178
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
import hashlib
import importlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import threading
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "tools"))

import gen_form_signals  # noqa: E402
import gen_signal_parts  # noqa: E402
import preview  # noqa: E402
import review  # noqa: E402


# A review baseline is evidence, not a disposable renderer cache.  Keep it in
# the ignored Cargo target tree so a terminal/browser restart or a later Codex
# turn cannot silently replace the immutable "before" pack.
DEFAULT_OUTPUT = ROOT / "target/signal-workbench"
WATCH_SOURCES = (
    ROOT / "tools/gen_form_signals.py",
    ROOT / "tools/gen_signal_parts.py",
    ROOT / "tools/signals/preview.py",
)


@dataclass(frozen=True)
class CaptureSpec:
    slug: str
    label: str
    aspect: str
    view: str
    focus: str
    background: str = "neutral"
    target_node: str | None = None
    isolate_target: bool = False
    transition: str | None = None


def capture_plan(
    profile: review.Profile,
    component: str,
    aspect: str | None = None,
    view: str | None = None,
    background: str | None = None,
    transition: str | None = None,
) -> tuple[CaptureSpec, ...]:
    """Return the small set that catches silhouette, attachment and motion drift."""
    known = next(
        (check for check in profile.components if check.node == component), None
    )
    detail_aspect = aspect or (known.aspect if known else profile.primary_aspect)
    detail_view = view or (known.view if known else "front")
    detail_background = background or (known.background if known else "neutral")
    motion = transition or (profile.transitions[0] if profile.transitions else None)
    captures = [
        CaptureSpec(
            "overall-front", "Gesamthöhe / Front", profile.primary_aspect,
            "front", "full",
        ),
        CaptureSpec(
            "context-front", "Kontext / Front", profile.primary_aspect,
            "front", "head",
        ),
        CaptureSpec(
            "context-side", "Kontext / Seite", profile.primary_aspect,
            "right", "head",
        ),
        CaptureSpec(
            "context-rear", "Kontext / Rückseite", profile.primary_aspect,
            "rear", "head", "light",
        ),
        CaptureSpec(
            "component-attached", "Bauteil eingebaut", detail_aspect,
            detail_view, "detail", detail_background, component,
        ),
        CaptureSpec(
            "component-isolated", "Bauteil freigestellt", detail_aspect,
            detail_view, "detail", detail_background, component, True,
        ),
    ]
    if motion:
        source, target = motion.split(":", 1)
        motion_detail = component.startswith("gewicht_ausgleich")
        captures.append(CaptureSpec(
            "motion", f"Animation {source} → {target}", target,
            detail_view if motion_detail else "front-left",
            "detail" if motion_detail else "head",
            detail_background if motion_detail else "neutral",
            component if motion_detail else None,
            transition=motion,
        ))
    return tuple(captures)


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def relative(path: Path, root: Path) -> str:
    return path.resolve().relative_to(root.resolve()).as_posix()


def read_json(path: Path) -> dict[str, object] | None:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None
    return value if isinstance(value, dict) else None


def dimensions(path: Path) -> dict[str, object] | None:
    data = read_json(path)
    if not data:
        return None
    framed = data.get("framed_size")
    assembly = data.get("assembly_size")
    if not (
        isinstance(framed, list) and len(framed) == 3
        and isinstance(assembly, list) and len(assembly) == 3
    ):
        return None
    return {
        "framed_cm": [round(float(value) * 100.0, 2) for value in framed],
        "assembly_cm": [round(float(value) * 100.0, 2) for value in assembly],
    }


def dimension_delta(
    before: dict[str, object] | None, current: dict[str, object] | None
) -> list[float] | None:
    if not before or not current:
        return None
    left = before.get("framed_cm")
    right = current.get("framed_cm")
    if not (isinstance(left, list) and isinstance(right, list) and len(left) == len(right)):
        return None
    return [round(float(b) - float(a), 2) for a, b in zip(left, right)]


class SignalWorkbench:
    def __init__(self, args: argparse.Namespace):
        self.profile_name = args.profile
        self.profile = review.PROFILES[args.profile]
        self.component = args.component
        self.kind = args.kind
        self.allowed_nodes = tuple(dict.fromkeys([self.component, *args.allow_node]))
        self.allowed_domains = review.EDIT_DOMAINS[self.kind]
        self.frames = args.frames
        self.jobs = args.jobs
        self.references = tuple(path.resolve() for path in args.reference)
        self.captures = capture_plan(
            self.profile,
            self.component,
            args.aspect,
            args.view,
            args.background,
            args.transition,
        )
        stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
        default = DEFAULT_OUTPUT / (
            f"{self.profile_name}-{self.component}-{stamp}-{os.getpid()}"
        )
        self.resuming = args.resume is not None
        self.root = (args.resume or args.output or default).resolve()
        if self.resuming:
            if not self.root.is_dir():
                raise RuntimeError(f"review session does not exist: {self.root}")
        else:
            self.root.mkdir(parents=True, exist_ok=False)
        self.lock = threading.Lock()
        self.render_lock = threading.Lock()
        self.stop_event = threading.Event()
        self.iteration = 0
        self.guard_manifest: Path | None = None
        self.baseline: list[dict[str, object]] = []
        self.parts_mtime = (ROOT / "tools/gen_signal_parts.py").stat().st_mtime_ns
        self.state: dict[str, object] = {
            "phase": "preparing",
            "message": "Arbeitskopie wird vorbereitet",
            "created_at": utc_now(),
            "updated_at": utc_now(),
            "revision": 0,
            "profile": self.profile_name,
            "model": self.profile.model,
            "component": self.component,
            "kind": self.kind,
            "allowed_nodes": list(self.allowed_nodes),
            "allowed_domains": list(self.allowed_domains),
            "captures": [],
            "references": [],
            "logs": [],
        }
        if not self.resuming:
            self._write_dashboard()
            self._write_state()

    def _write_state(self) -> None:
        with self.lock:
            self.state["updated_at"] = utc_now()
            temporary = self.root / ".state.json.tmp"
            temporary.write_text(
                json.dumps(self.state, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            temporary.replace(self.root / "state.json")

    def update(self, **values: object) -> None:
        with self.lock:
            self.state.update(values)
        self._write_state()

    def log(self, message: str) -> None:
        lines = message.rstrip().splitlines()
        with self.lock:
            logs = list(self.state.get("logs", []))
            logs.extend(lines)
            self.state["logs"] = logs[-120:]
        self._write_state()

    def command(self, command: list[str]) -> subprocess.CompletedProcess[str]:
        result = subprocess.run(
            command,
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        output = "\n".join(part for part in (result.stdout.strip(), result.stderr.strip()) if part)
        if output:
            self.log(output)
        if result.returncode:
            raise RuntimeError(
                f"command failed ({result.returncode}): {' '.join(command)}"
            )
        return result

    def reload_generator(self) -> None:
        global gen_form_signals, gen_signal_parts
        importlib.invalidate_caches()
        current_parts_mtime = (ROOT / "tools/gen_signal_parts.py").stat().st_mtime_ns
        if current_parts_mtime != self.parts_mtime:
            gen_signal_parts = importlib.reload(gen_signal_parts)
            self.parts_mtime = current_parts_mtime
        gen_form_signals = importlib.reload(gen_form_signals)

    def regenerate_model(self) -> None:
        self.reload_generator()
        gen_form_signals.validate_dimensions()
        gen_form_signals.generate_one(self.profile.model)

    def build_renderer(self) -> None:
        self.command(["cargo", "build", "-p", "signal-editor"])

    def validate_component_exists(self) -> None:
        model = preview.model_path(self.profile.model)
        fingerprints = preview.input_fingerprint_manifest(model)
        names = {
            node
            for asset in fingerprints["gltf_fingerprints"].values()
            for node in asset["node_geometry_sha256"]
        }
        missing = [prefix for prefix in self.allowed_nodes
                   if not any(name.startswith(prefix) for name in names)]
        if missing:
            raise RuntimeError(
                "node prefix not present in canonical model: " + ", ".join(missing)
            )

    def artifact_paths(
        self, directory: Path, capture: CaptureSpec
    ) -> tuple[Path, Path, Path | None]:
        if capture.transition:
            target = directory / capture.slug
            return target / "animation-strip.png", target / "manifest.json", None
        image = directory / f"{capture.slug}.png"
        return image, image.with_suffix(".json"), image.with_suffix(".bounds.json")

    def preview_command(
        self,
        directory: Path,
        capture: CaptureSpec,
        guard: Path | None,
    ) -> list[str]:
        image, _manifest, _bounds = self.artifact_paths(directory, capture)
        command = [
            sys.executable,
            str(ROOT / "tools/signals/preview.py"),
            self.profile.model,
            "--no-build",
            "--frames", str(self.frames),
            "--view", capture.view,
            "--focus", capture.focus,
            "--background", capture.background,
        ]
        if capture.transition:
            command += [
                "--animation", capture.transition,
                "--animation-times", review.QUICK_MOTION_TIMES,
                "--output", str(directory / capture.slug),
            ]
        else:
            command += ["--aspect", capture.aspect, "--output", str(image)]
            if capture.isolate_target:
                command.append("--isolate-target")
        if capture.target_node:
            command += ["--target-node", capture.target_node]
        if guard:
            command += ["--protect-component", str(guard)]
            for prefix in self.allowed_nodes:
                command += ["--allow-node", prefix]
            for domain in self.allowed_domains:
                command += ["--allow-domain", domain]
        return command

    def render_pack(
        self, directory: Path, guard: Path | None
    ) -> list[dict[str, object]]:
        directory.mkdir(parents=True, exist_ok=False)
        failures: list[str] = []
        with ThreadPoolExecutor(max_workers=self.jobs) as executor:
            pending = {
                executor.submit(
                    subprocess.run,
                    self.preview_command(directory, capture, guard),
                    cwd=ROOT,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                ): capture
                for capture in self.captures
            }
            for future in as_completed(pending):
                capture = pending[future]
                result = future.result()
                if result.returncode:
                    failures.append(
                        f"{capture.label}:\n{result.stdout}\n{result.stderr}".strip()
                    )
                else:
                    self.log(f"OK render: {capture.label}")
        if failures:
            raise RuntimeError("\n\n".join(failures))

        return self.read_pack(directory)

    def read_pack(self, directory: Path) -> list[dict[str, object]]:
        """Read one already-rendered pack without mutating its evidence."""
        output = []
        for capture in self.captures:
            image, manifest, bounds = self.artifact_paths(directory, capture)
            if not image.is_file() or not manifest.is_file():
                raise RuntimeError(f"missing render artifact for {capture.label}: {image}")
            output.append({
                "slug": capture.slug,
                "label": capture.label,
                "image": relative(image, self.root),
                "manifest": relative(manifest, self.root),
                "bounds": relative(bounds, self.root) if bounds and bounds.is_file() else None,
                "measurements": dimensions(bounds) if bounds else None,
                "spec": asdict(capture),
            })
        return output

    def copy_references(self) -> list[dict[str, object]]:
        target_dir = self.root / "references"
        target_dir.mkdir()
        copied = []
        for index, source in enumerate(self.references, 1):
            target = target_dir / f"{index:02d}-{source.name}"
            shutil.copy2(source, target)
            copied.append({
                "name": source.name,
                "image": relative(target, self.root),
                "sha256": file_sha256(target),
            })
        return copied

    def prepare(self) -> None:
        try:
            self.update(phase="preparing", message="Referenzsignal wird gezielt erzeugt")
            references = self.copy_references()
            self.regenerate_model()
            self.build_renderer()
            self.validate_component_exists()
            self.update(message="Unveränderliche Vorher-Aufnahmen werden gerendert")
            self.baseline = self.render_pack(self.root / "baseline", None)
            guard_entry = next(
                capture for capture in self.baseline if capture["slug"] == "context-front"
            )
            self.guard_manifest = self.root / str(guard_entry["manifest"])
            config = {
                "created_at": utc_now(),
                "profile": self.profile_name,
                "model": self.profile.model,
                "component": self.component,
                "kind": self.kind,
                "allowed_nodes": list(self.allowed_nodes),
                "allowed_domains": list(self.allowed_domains),
                "guard_manifest": relative(self.guard_manifest, self.root),
                "captures": [asdict(capture) for capture in self.captures],
                "references": references,
            }
            (self.root / "session.json").write_text(
                json.dumps(config, indent=2, sort_keys=True) + "\n",
                encoding="utf-8",
            )
            baseline_cards = [
                {
                    **capture,
                    "baseline": capture["image"],
                    "current": None,
                    "diff": None,
                    "rmse": None,
                    "delta_cm": None,
                }
                for capture in self.baseline
            ]
            self.update(
                phase="ready",
                message="Vorher-Zustand fixiert; warte auf Quelländerung",
                references=references,
                captures=baseline_cards,
                guard_manifest=relative(self.guard_manifest, self.root),
            )
        except BaseException as error:
            self.update(phase="failed", message=str(error))
            raise

    def resume_session(self) -> None:
        """Restore an immutable baseline after the workbench process stopped."""
        config = read_json(self.root / "session.json")
        previous_state = read_json(self.root / "state.json")
        if not config or not previous_state:
            raise RuntimeError(f"incomplete review session: {self.root}")
        expected = {
            "profile": self.profile_name,
            "model": self.profile.model,
            "component": self.component,
            "kind": self.kind,
            "allowed_nodes": list(self.allowed_nodes),
            "allowed_domains": list(self.allowed_domains),
            "captures": [asdict(capture) for capture in self.captures],
        }
        mismatches = [
            key for key, value in expected.items() if config.get(key) != value
        ]
        if mismatches:
            raise RuntimeError(
                "resume arguments do not match the fixed session: "
                + ", ".join(mismatches)
            )
        guard = config.get("guard_manifest")
        if not isinstance(guard, str):
            raise RuntimeError("review session has no guard manifest")
        self.guard_manifest = self.root / guard
        if not self.guard_manifest.is_file():
            raise RuntimeError(f"missing guard manifest: {self.guard_manifest}")
        self.baseline = self.read_pack(self.root / "baseline")
        iteration_dir = self.root / "iterations"
        completed = [
            int(path.name) for path in iteration_dir.iterdir()
            if path.is_dir() and path.name.isdigit()
        ] if iteration_dir.is_dir() else []
        self.iteration = max(
            completed + [int(previous_state.get("current_iteration", 0) or 0)]
        )
        self.state = previous_state
        self._write_dashboard()
        self.update(
            phase="ready",
            message=(
                f"Sitzung mit unveränderter Basis fortgesetzt; "
                f"nächste Iteration {self.iteration + 1}"
            ),
        )

    def preflight_guard(self) -> None:
        assert self.guard_manifest is not None
        model = preview.model_path(self.profile.model)
        current = preview.input_fingerprint_manifest(model)
        preview.verify_component_guard(
            current,
            self.guard_manifest,
            self.allowed_nodes,
            self.allowed_domains,
        )

    def compare_pack(
        self,
        current: list[dict[str, object]],
        iteration_dir: Path,
    ) -> list[dict[str, object]]:
        baseline_by_slug = {capture["slug"]: capture for capture in self.baseline}
        diff_dir = iteration_dir / "diff"
        diff_dir.mkdir()
        cards = []
        for capture in current:
            before = baseline_by_slug[capture["slug"]]
            before_image = self.root / str(before["image"])
            current_image = self.root / str(capture["image"])
            difference = diff_dir / f"{capture['slug']}.png"
            score = preview.compare(current_image, before_image, difference)
            cards.append({
                **capture,
                "baseline": before["image"],
                "current": capture["image"],
                "diff": relative(difference, self.root) if score > 0.0 else None,
                "rmse": round(score, 8),
                "baseline_measurements": before.get("measurements"),
                "delta_cm": dimension_delta(
                    before.get("measurements"), capture.get("measurements")
                ),
            })
        return cards

    def render_iteration(self, reason: str) -> None:
        if not self.render_lock.acquire(blocking=False):
            return
        try:
            self.iteration += 1
            number = self.iteration
            self.update(
                phase="rendering",
                revision=number,
                message=f"Iteration {number}: gezielte Erzeugung ({reason})",
            )
            self.regenerate_model()
            self.build_renderer()
            self.update(message=f"Iteration {number}: Komponenten-Sperre wird geprüft")
            try:
                self.preflight_guard()
            except SystemExit as error:
                self.update(
                    phase="blocked",
                    message=f"Iteration {number} vor dem Rendern blockiert: {error}",
                )
                return
            self.update(message=f"Iteration {number}: Bevy-Aufnahmen werden gerendert")
            iteration_dir = self.root / "iterations" / f"{number:04d}"
            current = self.render_pack(iteration_dir, self.guard_manifest)
            cards = self.compare_pack(current, iteration_dir)
            self.update(
                phase="passed",
                message=(
                    f"Iteration {number} bestanden: keine Änderung außerhalb "
                    f"von {', '.join(self.allowed_nodes)} / {self.kind}"
                ),
                captures=cards,
                current_iteration=number,
            )
        except BaseException as error:
            self.update(
                phase="failed",
                message=f"Iteration {self.iteration} fehlgeschlagen: {error}",
            )
        finally:
            self.render_lock.release()

    def trigger(self, reason: str) -> bool:
        if self.render_lock.locked():
            return False
        threading.Thread(
            target=self.render_iteration,
            args=(reason,),
            daemon=True,
            name="signal-workbench-render",
        ).start()
        return True

    def watch_signature(self) -> tuple[tuple[str, int, int], ...]:
        paths = list(WATCH_SOURCES)
        paths.extend(preview.renderer_sources())
        return tuple(
            (str(path), path.stat().st_mtime_ns, path.stat().st_size)
            for path in sorted(set(paths)) if path.is_file()
        )

    def watch(self, interval: float) -> None:
        signature = self.watch_signature()
        while not self.stop_event.wait(interval):
            current = self.watch_signature()
            if current != signature:
                signature = current
                # Editors often replace a file atomically. Give that rename and
                # formatter pass one short debounce window before importing it.
                time.sleep(0.45)
                self.trigger("Quelländerung")

    def _write_dashboard(self) -> None:
        (self.root / "index.html").write_text(DASHBOARD_HTML, encoding="utf-8")


class WorkbenchServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address: tuple[str, int], workbench: SignalWorkbench):
        self.workbench = workbench

        class Handler(SimpleHTTPRequestHandler):
            def __init__(handler_self, *values: object, **keywords: object):
                super().__init__(*values, directory=str(workbench.root), **keywords)

            def log_message(handler_self, _format: str, *_args: object) -> None:
                return

            def do_GET(handler_self) -> None:  # noqa: N802
                if urlparse(handler_self.path).path == "/api/state":
                    payload = json.dumps(workbench.state).encode("utf-8")
                    handler_self.send_response(200)
                    handler_self.send_header("Content-Type", "application/json; charset=utf-8")
                    handler_self.send_header("Cache-Control", "no-store")
                    handler_self.send_header("Content-Length", str(len(payload)))
                    handler_self.end_headers()
                    handler_self.wfile.write(payload)
                    return
                super().do_GET()

            def do_POST(handler_self) -> None:  # noqa: N802
                if urlparse(handler_self.path).path != "/api/render":
                    handler_self.send_error(404)
                    return
                accepted = workbench.trigger("manuell")
                payload = json.dumps({"accepted": accepted}).encode("utf-8")
                handler_self.send_response(202 if accepted else 409)
                handler_self.send_header("Content-Type", "application/json")
                handler_self.send_header("Content-Length", str(len(payload)))
                handler_self.end_headers()
                handler_self.wfile.write(payload)

        super().__init__(address, Handler)


DASHBOARD_HTML = r'''<!doctype html>
<html lang="de"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Formsignal-Arbeitsbank</title>
<style>
:root{color-scheme:dark;--bg:#101216;--panel:#1a1e24;--line:#343b46;--ink:#eef2f7;--muted:#aeb8c7;--ok:#4fd18b;--bad:#ff6b6b;--busy:#ffc857}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.45 system-ui,sans-serif}
header{position:sticky;top:0;z-index:4;display:flex;gap:18px;align-items:center;padding:14px 22px;background:#101216ed;border-bottom:1px solid var(--line);backdrop-filter:blur(12px)}
h1{margin:0;font-size:19px}.spacer{flex:1}.badge{padding:5px 10px;border-radius:99px;background:#333;color:var(--busy);font-weight:700}.badge.passed,.badge.ready{color:var(--ok)}.badge.failed,.badge.blocked{color:var(--bad)}
button{border:1px solid #657184;background:#273140;color:white;border-radius:6px;padding:8px 13px;font-weight:650;cursor:pointer}button:disabled{opacity:.45;cursor:wait}
main{padding:20px;max-width:1900px;margin:auto}.summary{display:grid;grid-template-columns:minmax(280px,1fr) minmax(340px,2fr);gap:16px;margin-bottom:18px}.panel{background:var(--panel);border:1px solid var(--line);border-radius:9px;padding:14px}
.meta{display:grid;grid-template-columns:max-content 1fr;gap:4px 12px;margin:0}.meta dt{color:var(--muted)}.meta dd{margin:0;font-family:ui-monospace,monospace;overflow-wrap:anywhere}.message{font-weight:650;margin-bottom:9px}
.refs{display:flex;gap:10px;overflow:auto}.refs img{height:190px;max-width:320px;object-fit:contain;background:#0b0d10;border:1px solid var(--line)}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(470px,1fr));gap:16px}.card{background:var(--panel);border:1px solid var(--line);border-radius:9px;padding:12px;min-width:0}.card h2{font-size:16px;margin:0 0 8px}.images{display:grid;grid-template-columns:1fr 1fr;gap:8px}.shot{min-width:0}.shot b{display:block;color:var(--muted);font-size:12px;margin-bottom:4px}.shot img{width:100%;height:330px;display:block;object-fit:contain;background:#0b0d10;border:1px solid var(--line)}
.detail{display:flex;gap:14px;flex-wrap:wrap;color:var(--muted);font:12px ui-monospace,monospace;margin-top:8px}.diff{margin-top:8px}.diff img{width:100%;max-height:240px;object-fit:contain;background:#0b0d10;border:1px solid var(--line)}details{margin-top:18px}.logs{white-space:pre-wrap;max-height:280px;overflow:auto;color:#c9d2df;font:12px/1.45 ui-monospace,monospace}
@media(max-width:850px){.summary{grid-template-columns:1fr}.grid{grid-template-columns:1fr}.images{grid-template-columns:1fr}.shot img{height:auto}}
</style></head><body>
<header><h1>Formsignal-Arbeitsbank</h1><span id="badge" class="badge">lädt</span><div class="spacer"></div><button id="render">Jetzt neu rendern</button></header>
<main><section class="summary"><div class="panel"><div id="message" class="message"></div><dl id="meta" class="meta"></dl></div><div class="panel"><b>Vorbildmaterial</b><div id="refs" class="refs"></div></div></section><section id="grid" class="grid"></section><details class="panel"><summary>Protokoll</summary><pre id="logs" class="logs"></pre></details></main>
<script>
let stamp=''; const esc=s=>String(s??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const img=(path,alt,rev)=>path?`<a href="${esc(path)}?v=${rev}"><img src="${esc(path)}?v=${rev}" alt="${esc(alt)}"></a>`:'<div>noch keine Aufnahme</div>';
function measurements(c){const m=c.measurements?.framed_cm,d=c.delta_cm;return `${m?`Rahmen X/Y/Z: ${m.join(' / ')} cm`:''}${d?` · Δ ${d.map(v=>(v>=0?'+':'')+v).join(' / ')} cm`:''}${c.rmse!==null&&c.rmse!==undefined?` · RMSE ${c.rmse}`:''}`}
async function refresh(){let s;try{s=await(await fetch('/api/state',{cache:'no-store'})).json()}catch(e){return}const key=s.updated_at+':'+s.phase;if(key===stamp)return;stamp=key;const b=document.querySelector('#badge');b.textContent=s.phase;b.className='badge '+s.phase;document.querySelector('#message').textContent=s.message||'';document.querySelector('#render').disabled=s.phase==='rendering'||s.phase==='preparing';document.querySelector('#meta').innerHTML=`<dt>Modell</dt><dd>${esc(s.model)}</dd><dt>Bauteil</dt><dd>${esc((s.allowed_nodes||[]).join(', '))}</dd><dt>Änderungsart</dt><dd>${esc(s.kind)} (${esc((s.allowed_domains||[]).join(', '))})</dd><dt>Guard</dt><dd>${esc(s.guard_manifest||'wird erstellt')}</dd>`;document.querySelector('#refs').innerHTML=(s.references||[]).map(r=>img(r.image,r.name,s.revision)).join('')||'<span>Kein Vorbildbild hinterlegt</span>';document.querySelector('#grid').innerHTML=(s.captures||[]).map(c=>`<article class="card"><h2>${esc(c.label)}</h2><div class="images"><div class="shot"><b>VORHER (fixiert)</b>${img(c.baseline,c.label+' vorher',s.revision)}</div><div class="shot"><b>AKTUELL</b>${img(c.current,c.label+' aktuell',s.revision)}</div></div><div class="detail">${esc(measurements(c))}</div>${c.diff?`<div class="diff"><b>PIXEL-DIFFERENZ</b>${img(c.diff,c.label+' Differenz',s.revision)}</div>`:''}</article>`).join('');document.querySelector('#logs').textContent=(s.logs||[]).join('\n')}
document.querySelector('#render').onclick=async()=>{document.querySelector('#render').disabled=true;await fetch('/api/render',{method:'POST'});setTimeout(refresh,150)};setInterval(refresh,900);refresh();
</script></body></html>'''


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profile", choices=tuple(review.PROFILES))
    parser.add_argument("--component", required=True, help="glTF node-name prefix under review")
    parser.add_argument(
        "--kind", choices=tuple(review.EDIT_DOMAINS), default="geometry",
        help="only this domain may change on the selected component",
    )
    parser.add_argument(
        "--allow-node", action="append", default=[], metavar="PREFIX",
        help="additional node family intentionally owned by this edit",
    )
    parser.add_argument("--reference", type=Path, action="append", default=[])
    parser.add_argument("--aspect")
    parser.add_argument("--view", choices=preview.ALL_VIEWS)
    parser.add_argument("--background", choices=("neutral", "light", "dark"))
    parser.add_argument("--transition", help="FROM:TO animation shown in the pack")
    parser.add_argument("--frames", type=int, default=35)
    parser.add_argument("--jobs", type=int, default=3)
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=4178)
    parser.add_argument("--output", type=Path)
    parser.add_argument(
        "--resume", type=Path, metavar="SESSION",
        help="restart an existing persistent session without replacing its baseline",
    )
    parser.add_argument("--no-watch", action="store_true")
    args = parser.parse_args()
    if args.output and args.resume:
        parser.error("--output and --resume are mutually exclusive")
    if args.resume and args.reference:
        parser.error("a resumed session keeps its fixed references; omit --reference")
    if not 1 <= args.jobs <= 4:
        parser.error("--jobs must be between 1 and 4")
    if args.frames < 1:
        parser.error("--frames must be positive")
    if not 1 <= args.port <= 65535:
        parser.error("--port must be between 1 and 65535")
    missing = [path for path in args.reference if not path.is_file()]
    if missing:
        parser.error("missing reference image(s): " + ", ".join(map(str, missing)))
    valid_aspects = preview.aspects_for(review.PROFILES[args.profile].model)
    if args.aspect and args.aspect not in valid_aspects:
        parser.error(f"--aspect must be one of: {', '.join(valid_aspects)}")
    if args.transition:
        try:
            source, target = args.transition.split(":", 1)
        except ValueError:
            parser.error("--transition must be FROM:TO")
        if source not in valid_aspects or target not in valid_aspects:
            parser.error(f"--transition aspects must be among: {', '.join(valid_aspects)}")
    return args


def main() -> None:
    args = parse_args()
    workbench = SignalWorkbench(args)
    if workbench.resuming:
        workbench.resume_session()
    else:
        workbench.prepare()
    if not args.no_watch:
        threading.Thread(
            target=workbench.watch,
            args=(0.75,),
            daemon=True,
            name="signal-workbench-watch",
        ).start()
    server = WorkbenchServer((args.host, args.port), workbench)
    print(f"Signal workbench: http://127.0.0.1:{args.port}/")
    print(f"Session: {workbench.root}")
    try:
        server.serve_forever(poll_interval=0.25)
    except KeyboardInterrupt:
        pass
    finally:
        workbench.stop_event.set()
        server.server_close()


if __name__ == "__main__":
    main()
