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
    modified = original_helper
    modified = modified.replace(".add_systems(PostUpdate, qa::capture", ".add_systems(Update, qa::display.after(audition::update).before(ui::update))\n    .add_systems(PostUpdate, qa::capture", 1)
    copy.write_text(modified)
    original = (common.SOURCE / "workbench.rs").read_text()
    patch = output / "instrumentation.patch"
    patch.write_text("".join(difflib.unified_diff(original.splitlines(keepends=True), modified.splitlines(keepends=True), fromfile="production/workbench.rs", tofile="helper/workbench.rs")))
    record = json.loads((output / "prepared.json").read_text())
    record.update({"helper_source": common.tree(output / "source"), "instrumentation_sha256": common.digest(patch),
                   "extra_harness_sha256": {str(HERE / name): common.digest(HERE / name) for name in HARNESS},
                   "scope": "Eight copied production modules; window size and QA systems injected; display-only mode or synthetic input through unchanged real audio path"})
    common.save(output / "prepared.json", record)


def verify(output):
    record = common.verify_source(output)
    for name, sha in record["extra_harness_sha256"].items():
        assert common.digest(Path(name)) == sha, f"Audition harness changed: {name}"
    return record


def build_frozen(output, freeze):
    prepared = verify(output)
    record = json.loads(freeze.read_text())
    assert prepared["production_source"] == record["source"]
    for name, item in record["dependency_artifacts"].items():
        assert common.digest(freeze.parent / "frozen-deps" / name) == item["sha256"]
    for name, item in record["native_archives"].items():
        assert common.digest(Path(name)) == item["sha256"]
    production = Path(record["production_binary"])
    assert common.digest(production) == record["production_binary_sha256"]
    binary = output / "workbench-audition-check"
    assert not binary.exists()
    command = ["rustc", "--edition=2024", "--crate-name=workbench_audition_check", str(output / "source/main.rs"), "-o", str(binary), "-L", "dependency=" + str(freeze.parent / "frozen-deps")]
    for path in record["frozen_native_paths"]:
        command.extend(["-L", path])
    for name in sorted(common.EXTERNS):
        command.extend(["--extern", name + "=" + record["externs"][name]])
    env = os.environ.copy()
    env["CARGO_MANIFEST_DIR"] = str(ROOT / "tools/cocobeat-lab")
    with (output / "build.log").open("xb") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    common.save(output / "build.json", {"command": command, "exit_code": result.returncode,
                "freeze": str(freeze), "freeze_sha256": common.digest(freeze), "binary": str(binary),
                "binary_sha256": common.digest(binary) if binary.exists() else None,
                "production_binary": str(production), "production_binary_sha256": common.digest(production),
                "externs": {name: {"path": record["externs"][name], "sha256": common.digest(Path(record["externs"][name]))} for name in common.EXTERNS}})
    assert result.returncode == 0, f"Frozen helper build failed: {output / 'build.log'}"


def run_audio(output, package):
    verify(output)
    build = json.loads((output / "build.json").read_text())
    assert build["exit_code"] == 0
    assert common.digest(Path(build["binary"])) == build["binary_sha256"]
    source = common.tree(package)
    recording = output / "readonly-negative-selection.json"
    command = [build["binary"], "make-replay", str(package), str(recording)]
    with (output / "make-replay.log").open("xb") as log:
        subprocess.run(command, check=True, stdout=log, stderr=subprocess.STDOUT)
    recording_sha = common.digest(recording)
    result = {"package": str(package), "package_sha256": source, "recording_sha256": recording_sha,
              "scope": "Synthetic KeyboardInput -> unchanged workbench capture -> audition Output -> native Kira CPAL source cursor/state; no injected playback state or position",
              "physical_input_acoustic_output_latency_human": "NOT RUN", "cases": []}
    for width, unavailable in [(1280, False), (640, False), (640, True)]:
        height = 800 if width == 1280 else 480
        case = output / (f"audio-{width}" if not unavailable else "unavailable-output")
        case.mkdir()
        with tempfile.TemporaryDirectory(prefix="ccba-") as directory:
            os.chmod(directory, 0o700)
            env = os.environ.copy()
            env.pop("DISPLAY", None)
            env.pop("WAYLAND_DISPLAY", None)
            audio_runtime = os.environ.get("PIPEWIRE_RUNTIME_DIR", os.environ.get("XDG_RUNTIME_DIR", ""))
            env.update({"XDG_RUNTIME_DIR": directory, "WINIT_UNIX_BACKEND": "x11", "DISABLE_GAMESCOPE_WSI": "1", "WGPU_BACKEND": "vulkan", "QA_SIZE": str(width), "QA_CAPTURE_DIR": str(case), "QA_REAL_AUDIO": "1", "PIPEWIRE_RUNTIME_DIR": audio_runtime})
            if unavailable:
                config = case / "empty-alsa.conf"
                config.touch(exist_ok=False)
                env.update({"QA_UNAVAILABLE_AUDIO": "1", "ALSA_CONFIG_PATH": str(config)})
            command = ["gamescope", "--backend", "headless", "-W", str(width), "-H", str(height), "-w", str(width), "-h", str(height), "--", build["binary"], str(package), str(recording)]
            timed_out = False
            with (case / "run.log").open("xb") as log:
                process = subprocess.Popen(command, cwd=case, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    process.wait(timeout=45)
                except subprocess.TimeoutExpired:
                    timed_out = True
                finally:
                    cleanup = common.stop_group(process)
        events = [json.loads(line[3:]) for line in (case / "run.log").read_text(errors="replace").splitlines() if line.startswith("QA ")]
        result["cases"].append({"command": command, "size": [width, height], "output_unavailable_control": unavailable, "exit_code": process.returncode, "timed_out": timed_out, "owned_pid": process.pid, "cleanup": cleanup, "events": events, "run_log_sha256": common.digest(case / "run.log"), "png_sha256": {p.name: common.digest(p) for p in sorted(case.glob("*.png"))}})
        common.save(output / "run-audio.json", result)
        common.unchanged(package, source)
        assert common.digest(recording) == recording_sha
        verify(output)
        assert not timed_out and process.returncode == 0, f"Real audio helper failed: {case / 'run.log'}"
        assert any(event.get("event") == "finished" for event in events)
        assert set(result["cases"][-1]["png_sha256"]) == ({"output-error-preserved.png"} if unavailable else {"paused-seek-no-playback.png", "resumed-new-source-start.png", "stopped-selection-preserved.png"})
        print(f"REAL_AUDIO_SOFTWARE_PASS {width}x{height} unavailable={unavailable}", flush=True)


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
    for name in ["prepare", "build", "build-frozen", "run", "run-audio"]:
        action = actions.add_parser(name)
        action.add_argument("output", type=Path)
        if name == "build":
            action.add_argument("cargo_json", type=Path)
            action.add_argument("source_manifest", type=Path)
        if name == "build-frozen":
            action.add_argument("freeze", type=Path)
        if name in ("run", "run-audio"):
            action.add_argument("package", type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    if args.action == "prepare":
        prepare(output)
    elif args.action == "build":
        verify(output)
        common.build(output, args.cargo_json.resolve(), args.source_manifest.resolve())
    elif args.action == "build-frozen":
        build_frozen(output, args.freeze.resolve())
    elif args.action == "run-audio":
        run_audio(output, args.package.resolve())
    else:
        run(output, args.package.resolve())
