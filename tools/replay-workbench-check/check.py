"""Run the production Replay workbench in an isolated display with synthetic input"""

import argparse
import difflib
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "tools/cocobeat-lab/src"
COPIES = ("replay.rs", "workbench.rs", "workbench/input.rs", "workbench/ui.rs", "workbench/replay.rs")
EXTERNS = {"bevy", "serde_json", "cocobeat_schema", "cocobeat_media", "cocobeat_editor", "cocobeat_runtime", "cocobeat_core", "cocobeat_replay"}
PNGS = {"negative-watermark-details.png", "wave-list-tail.png", "pair-wave-list.png", "pair-details-top.png", "pair-details-bottom.png"}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def tree(directory):
    return {str(path.relative_to(directory)): digest(path) for path in sorted(directory.rglob("*")) if path.is_file()}


def unchanged(directory, expected):
    assert tree(directory) == expected, f"Files changed in {directory}"


def prepare(output):
    output.mkdir()
    (output / "source/workbench").mkdir(parents=True)
    manifest = {}
    for name in COPIES:
        data = (SOURCE / name).read_bytes()
        manifest[name] = hashlib.sha256(data).hexdigest()
        (output / "source" / name).write_bytes(data)
    for name, destination in [("main.rs", "main.rs"), ("qa.rs", "workbench/qa.rs")]:
        (output / "source" / destination).write_bytes(Path(__file__).with_name(name).read_bytes())
    copy = output / "source/workbench.rs"
    original = copy.read_text()
    modified = original
    for before, after in [
        ("mod ui;", "mod ui;\nmod qa;"),
        ("resolution: (1280, 800).into(),", "resolution: qa::size().into(),"),
        (".add_systems(Update, (poll_save, input::capture, ui::update).chain());",
         ".add_systems(Update, (poll_save, input::capture, ui::update).chain())\n"
         "    .init_resource::<qa::Driver>()\n"
         "    .add_systems(Update, qa::drive.before(input::capture))\n"
         "    .add_systems(PostUpdate, qa::capture.after(bevy::ui::UiSystems::Layout));"),
    ]:
        assert modified.count(before) == 1, f"Instrumentation anchor changed: {before}"
        modified = modified.replace(before, after, 1)
    copy.write_text(modified)
    (output / "instrumentation.patch").write_text("".join(difflib.unified_diff(
        original.splitlines(keepends=True), modified.splitlines(keepends=True),
        fromfile="production/workbench.rs", tofile="helper/workbench.rs")))
    save(output / "prepared.json", {
        "git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "production_source": manifest,
        "helper_source": tree(output / "source"),
        "harness_source": {name: digest(Path(__file__).with_name(name)) for name in ("check.py", "main.rs", "qa.rs")},
        "instrumentation_sha256": digest(output / "instrumentation.patch"),
        "scope": "Copied production modules; only window size and QA systems are injected into workbench.rs",
    })
    print(f"PREPARED {output}")


def verify_source(output):
    prepared = json.loads((output / "prepared.json").read_text())
    for name, sha in prepared["production_source"].items():
        assert digest(SOURCE / name) == sha, f"Production source changed: {name}"
    unchanged(output / "source", prepared["helper_source"])
    assert digest(output / "instrumentation.patch") == prepared["instrumentation_sha256"]
    for name, sha in prepared["harness_source"].items():
        assert digest(Path(__file__).with_name(name)) == sha, f"Harness changed: {name}"
    return prepared


def build(output, cargo_json, source_record):
    prepared = verify_source(output)
    production_source = json.loads(source_record.read_text())
    required = {"Cargo.toml", "Cargo.lock", ".cargo/config.toml", "tools/cocobeat-lab/Cargo.toml"}
    required.update("tools/cocobeat-lab/src/" + name for name in COPIES)
    required.update(str(path.relative_to(ROOT)) for path in (ROOT / "crates").glob("*/Cargo.toml"))
    required.update(str(path.relative_to(ROOT)) for path in (ROOT / "crates").glob("*/src/lib.rs"))
    assert required <= production_source.keys(), "The build source manifest is incomplete"
    compiler_inputs = {name: sha for name, sha in production_source.items()
                       if name in required or name.startswith(("crates/", "tools/cocobeat-lab/", "assets/i18n/"))}
    for name, sha in compiler_inputs.items():
        assert digest(ROOT / name) == sha, f"Build source changed: {name}"
    externs = {}
    native = set()
    production = None
    finished = False
    for line in cargo_json.read_text().splitlines():
        item = json.loads(line)
        if item.get("reason") == "build-finished":
            finished = item["success"]
        if item.get("reason") == "build-script-executed":
            native.update(item["linked_paths"])
        if item.get("reason") != "compiler-artifact":
            continue
        name = item["target"]["name"]
        if name == "cocobeat-lab" and item.get("executable") and not item["profile"]["test"]:
            production = Path(item["executable"])
        if name in EXTERNS and not item["profile"]["test"]:
            for file in item["filenames"]:
                if file.endswith(".rlib"):
                    assert name not in externs or externs[name] == file, f"Ambiguous artifact: {name}"
                    externs[name] = file
    assert finished and set(externs) == EXTERNS and production is not None
    binary = output / "replay-workbench-check"
    assert not binary.exists(), "Use a new output directory"
    command = ["rustc", "--edition=2024", "--crate-name=replay_workbench_check", str(output / "source/main.rs"), "-o", str(binary)]
    for directory in sorted({str(Path(file).parent) for file in externs.values()}):
        command.extend(["-L", "dependency=" + directory])
    for path in sorted(native):
        command.extend(["-L", path])
    for name, file in sorted(externs.items()):
        command.extend(["--extern", name + "=" + file])
    env = os.environ.copy()
    env["CARGO_MANIFEST_DIR"] = str(ROOT / "tools/cocobeat-lab")
    with (output / "build.log").open("wb") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    report = {
        "command": command, "cwd": str(ROOT), "exit_code": result.returncode,
        "cargo_json": str(cargo_json), "cargo_json_sha256": digest(cargo_json),
        "prepared": prepared,
        "externs": {name: {"path": file, "sha256": digest(Path(file))} for name, file in externs.items()},
        "binary": str(binary), "binary_sha256": digest(binary) if binary.exists() else None,
        "production_binary": str(production), "production_binary_sha256": digest(production),
    }
    report["production_build_source"] = {"path": str(source_record), "sha256": digest(source_record), "record": production_source, "verified_compiler_inputs": compiler_inputs}
    save(output / "build.json", report)
    assert result.returncode == 0, f"Helper compilation failed: {output / 'build.log'}"
    print(f"BUILT {binary}")


def stop_group(process):
    # The compositor can exit before its helper or Xwayland child
    signals = []
    for signum, grace in [(signal.SIGTERM, 5.0), (signal.SIGKILL, 0.0)]:
        try:
            os.killpg(process.pid, signum)
            signals.append(signum.name)
        except ProcessLookupError:
            break
        deadline = time.monotonic() + grace
        while time.monotonic() < deadline:
            process.poll()
            try:
                os.killpg(process.pid, 0)
            except ProcessLookupError:
                break
            time.sleep(0.05)
        else:
            continue
        break
    process.wait()
    return signals


def run(output, package, recording, report):
    verify_source(output)
    build_record = json.loads((output / "build.json").read_text())
    helper = Path(build_record["binary"])
    assert digest(helper) == build_record["binary_sha256"]
    assert digest(Path(build_record["production_binary"])) == build_record["production_binary_sha256"]
    originals = {"package": tree(package), "replay": digest(recording), "report": digest(report)}
    result = {"build": build_record, "package": str(package), "replay": str(recording), "expected_report": str(report), "original_sha256": originals,
              "input_basis": "Synthetic Bevy KeyboardInput messages through unchanged production input::capture",
              "render_basis": "Native Bevy Screenshot GPU readback in an owned headless gamescope display",
              "physical_input_audio_human_acceptance": "NOT RUN", "visual_review": "NOT RUN", "cases": []}
    for width, height in [(1280, 800), (640, 480)]:
        case = output / f"case-{width}"
        case.mkdir()
        runtime = case / "runtime"
        runtime.mkdir(mode=0o700)
        env = os.environ.copy()
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)
        env.update({"XDG_RUNTIME_DIR": str(runtime), "WINIT_UNIX_BACKEND": "x11", "DISABLE_GAMESCOPE_WSI": "1", "WGPU_BACKEND": "vulkan",
                    "QA_SIZE": str(width), "QA_CAPTURE_DIR": str(case), "QA_REPORT": str(report)})
        command = ["gamescope", "--backend", "headless", "-W", str(width), "-H", str(height), "-w", str(width), "-h", str(height),
                   "--", str(helper), str(package), str(recording)]
        timed_out = False
        with (case / "run.log").open("wb") as log:
            process = subprocess.Popen(command, cwd=case, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            try:
                process.wait(timeout=60)
            except subprocess.TimeoutExpired:
                timed_out = True
            finally:
                cleanup_signals = stop_group(process)
        events = [json.loads(line[3:]) for line in (case / "run.log").read_text(errors="replace").splitlines() if line.startswith("QA ")]
        item = {"size": [width, height], "command": command, "cwd": str(case), "owned_process_group": process.pid,
                "exit_code": process.returncode, "timed_out": timed_out, "cleanup_signals": cleanup_signals, "events": events,
                "png_sha256": {path.name: digest(path) for path in sorted(case.glob("*.png"))}}
        result["cases"].append(item)
        unchanged(package, originals["package"])
        assert digest(recording) == originals["replay"] and digest(report) == originals["report"]
        save(output / "run.json", result)
        assert not timed_out and process.returncode == 0, f"QA process failed: {case / 'run.log'}"
        assert set(item["png_sha256"]) == PNGS and any(event.get("event") == "finished" for event in events)
        print(f"AUTOMATED_CAPTURE_PASS {width}x{height}; visual review remains required", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    for action in ("prepare", "build", "run"):
        command = actions.add_parser(action)
        command.add_argument("output", type=Path)
        if action == "build":
            command.add_argument("cargo_json", type=Path)
            command.add_argument("source_manifest", type=Path)
        if action == "run":
            for name in ("package", "recording", "report"):
                command.add_argument(name, type=Path)
    args = parser.parse_args()
    if args.action == "prepare":
        prepare(args.output.resolve())
    elif args.action == "build":
        build(args.output.resolve(), args.cargo_json.resolve(), args.source_manifest.resolve())
    else:
        run(args.output.resolve(), args.package.resolve(), args.recording.resolve(), args.report.resolve())
