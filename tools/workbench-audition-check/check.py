"""Capture minimal audition UI controls without claiming measured audio playback"""

import argparse
import difflib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("workbench_common", ROOT / "tools/replay-workbench-check/check.py")
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)
HERE = Path(__file__).parent
HARNESS = ("check.py", "main.rs", "qa.rs", "README.md")


def prepare(output):
    common.prepare(output)
    for name, destination in [("main.rs", "main.rs"), ("qa.rs", "workbench/qa.rs")]:
        (output / "source" / destination).write_bytes((HERE / name).read_bytes())
    copy = output / "source/workbench.rs"
    original_helper = copy.read_text()
    assert original_helper.count("qa::drive.before(input::capture)") == 1
    modified = original_helper.replace("qa::drive.before(input::capture)", "qa::drive.after(audition::update).before(ui::update)", 1)
    copy.write_text(modified)
    original = (common.SOURCE / "workbench.rs").read_text()
    patch = output / "instrumentation.patch"
    patch.write_text("".join(difflib.unified_diff(original.splitlines(keepends=True), modified.splitlines(keepends=True), fromfile="production/workbench.rs", tofile="helper/workbench.rs")))
    record = json.loads((output / "prepared.json").read_text())
    record.update({"helper_source": common.tree(output / "source"), "instrumentation_sha256": common.digest(patch),
                   "extra_harness_sha256": {str(HERE / name): common.digest(HERE / name) for name in HARNESS},
                   "scope": "Eight copied production modules; window size and QA systems injected; explicit synthetic audition display, no output-device playback"})
    common.save(output / "prepared.json", record)


def verify(output):
    record = common.verify_source(output)
    for name, sha in record["extra_harness_sha256"].items():
        assert common.digest(Path(name)) == sha, f"Audition harness changed: {name}"
    return record


def run(output, package):
    verify(output)
    build = json.loads((output / "build.json").read_text())
    assert build["exit_code"] == 0
    for artifact in [build["binary"], build["production_binary"]]:
        key = "binary_sha256" if artifact == build["binary"] else "production_binary_sha256"
        assert common.digest(Path(artifact)) == build[key]
    for item in build["externs"].values():
        assert common.digest(Path(item["path"])) == item["sha256"]
    source = common.tree(package)
    result = {"package": str(package), "package_sha256": source, "scope": "Synthetic UI cursor, native GPU layout; actual Kira MockBackend checked separately", "physical_audio_input_human": "NOT RUN", "visual_review": "NOT RUN", "cases": []}
    for width, height in [(1280, 800), (640, 480)]:
        case = output / f"case-{width}"
        case.mkdir()
        with tempfile.TemporaryDirectory(prefix="ccba-") as directory:
            os.chmod(directory, 0o700)
            env = os.environ.copy()
            env.pop("DISPLAY", None)
            env.pop("WAYLAND_DISPLAY", None)
            env.update({"XDG_RUNTIME_DIR": directory, "WINIT_UNIX_BACKEND": "x11", "DISABLE_GAMESCOPE_WSI": "1", "WGPU_BACKEND": "vulkan", "QA_SIZE": str(width), "QA_CAPTURE_DIR": str(case)})
            command = ["gamescope", "--backend", "headless", "-W", str(width), "-H", str(height), "-w", str(width), "-h", str(height), "--", build["binary"], str(package), str(case / "unwritten-package")]
            timed_out = False
            with (case / "run.log").open("wb") as log:
                process = subprocess.Popen(command, cwd=case, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    process.wait(timeout=40)
                except subprocess.TimeoutExpired:
                    timed_out = True
                finally:
                    cleanup = common.stop_group(process)
        events = [json.loads(line[3:]) for line in (case / "run.log").read_text(errors="replace").splitlines() if line.startswith("QA ")]
        item = {"command": command, "size": [width, height], "exit_code": process.returncode, "timed_out": timed_out, "owned_pid": process.pid, "cleanup": cleanup, "events": events,
                "png_sha256": {p.name: common.digest(p) for p in sorted(case.glob("*.png"))}}
        result["cases"].append(item)
        common.save(output / "run.json", result)
        common.unchanged(package, source)
        verify(output)
        assert not (case / "unwritten-package").exists()
        assert not timed_out and process.returncode == 0, f"Helper failed: {case / 'run.log'}"
        assert set(item["png_sha256"]) == {"idle-toolbar.png", "independent-audio-cursor.png"}
        assert any(event.get("event") == "finished" for event in events)
        print(f"AUTOMATED_CAPTURE_PASS {width}x{height}; visual review required", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    for name in ["prepare", "build", "run"]:
        action = actions.add_parser(name)
        action.add_argument("output", type=Path)
        if name == "build":
            action.add_argument("cargo_json", type=Path)
            action.add_argument("source_manifest", type=Path)
        if name == "run":
            action.add_argument("package", type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    if args.action == "prepare":
        prepare(output)
    elif args.action == "build":
        verify(output)
        common.build(output, args.cargo_json.resolve(), args.source_manifest.resolve())
    else:
        run(output, args.package.resolve())
