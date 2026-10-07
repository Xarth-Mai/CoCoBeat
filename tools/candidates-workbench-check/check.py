"""Capture the production candidate workbench with owned synthetic-input QA"""

import argparse
import difflib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import struct
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
COMMON_PATH = ROOT / "tools/replay-workbench-check/check.py"
spec = importlib.util.spec_from_file_location("replay_workbench_check", COMMON_PATH)
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)
digest, save, tree, stop_group = common.digest, common.save, common.tree, common.stop_group
SOURCE = ROOT / "tools/cocobeat-lab/src"
COPIES = common.COPIES
EXTERNS = common.EXTERNS
HARNESS = ("check.py", "main.rs", "fixture.rs", "qa.rs", "README.md")
RICH_PNGS = {"unknown-details.png", "dense-blocker-details.png", "accepted-wave-list.png", "list-tail.png", "details-bottom.png"}
EMPTY_PNGS = {"zero-candidates.png"}
AUDIO = ROOT / "testdata/synthetic/media-import/stereo-canonical.ogg"


def checked_hashes(paths):
    return {str(path): digest(path) for path in paths}


def verify_hashes(expected):
    for name, sha in expected.items():
        assert digest(Path(name)) == sha, f"Input changed: {name}"


def prepare(output):
    output.mkdir()
    (output / "source/workbench").mkdir(parents=True)
    originals = checked_hashes(SOURCE / name for name in COPIES)
    for name in COPIES:
        (output / "source" / name).write_bytes((SOURCE / name).read_bytes())
    for name, destination in [("main.rs", "main.rs"), ("fixture.rs", "fixture.rs"), ("qa.rs", "workbench/qa.rs")]:
        (output / "source" / destination).write_bytes(Path(__file__).with_name(name).read_bytes())
    copy = output / "source/workbench.rs"
    original = copy.read_text()
    modified = original
    for before, after in [
        ("mod ui;", "mod ui;\nmod qa;"),
        ("resolution: (1280, 800).into(),", "resolution: qa::size().into(),"),
        (".add_systems(\n            Update,\n            (poll_save, input::capture, audition::update, ui::update).chain(),\n        );",
         ".add_systems(Update, (poll_save, input::capture, audition::update, ui::update).chain())\n"
         "    .init_resource::<qa::Driver>()\n"
         "    .add_systems(Update, qa::drive.before(input::capture))\n"
         "    .add_systems(PostUpdate, qa::capture.after(bevy::ui::UiSystems::Layout));"),
    ]:
        assert modified.count(before) == 1, f"Instrumentation anchor changed: {before}"
        modified = modified.replace(before, after, 1)
    copy.write_text(modified)
    patch = output / "instrumentation.patch"
    patch.write_text("".join(difflib.unified_diff(
        original.splitlines(keepends=True), modified.splitlines(keepends=True),
        fromfile="production/workbench.rs", tofile="helper/workbench.rs")))
    inputs = originals | checked_hashes([COMMON_PATH, AUDIO])
    inputs.update(checked_hashes(Path(__file__).parent / name for name in HARNESS))
    inputs.update(checked_hashes(sorted((ROOT / "assets/i18n").glob("*.json"))))
    save(output / "prepared.json", {
        "git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "input_sha256": inputs, "helper_source": tree(output / "source"),
        "instrumentation_sha256": digest(patch),
        "scope": "Eight copied production modules; workbench.rs changes only window size and QA systems; fixture features are constructed uncalibrated controls, not MIR output",
    })
    print(f"PREPARED {output}", flush=True)


def verify_source(output):
    prepared = json.loads((output / "prepared.json").read_text())
    verify_hashes(prepared["input_sha256"])
    assert tree(output / "source") == prepared["helper_source"], "Helper source changed"
    assert digest(output / "instrumentation.patch") == prepared["instrumentation_sha256"]
    return prepared


def verify_build(output):
    verify_source(output)
    record = json.loads((output / "build.json").read_text())
    assert record["exit_code"] == 0, "Helper build did not succeed"
    verify_hashes(record["dependency_input_sha256"])
    verify_hashes(record["verified_lab_and_catalog_sha256"])
    assert digest(Path(record["binary"])) == record["binary_sha256"], "Helper binary changed"
    return record


def build(output, cargo_json, basis_path):
    prepared = verify_source(output)
    assert not (output / "build.json").exists(), "Use a new evidence directory"
    basis = json.loads(basis_path.read_text())
    assert basis["exit_code"] == 0, "Dependency-basis test compilation failed"
    assert Path(basis["cargo_json"]).resolve() == cargo_json
    assert digest(cargo_json) == basis["cargo_json_sha256"]
    source = basis["source"] | basis["locales"]
    required = {"tools/cocobeat-lab/src/" + name for name in COPIES}
    assert required <= source.keys(), "Dependency basis lacks copied production source hashes"
    assert set(basis["locales"]) == {str(path.relative_to(ROOT)) for path in (ROOT / "assets/i18n").glob("*.json")}
    verified_source = {str(ROOT / name): sha for name, sha in source.items()}
    verify_hashes(verified_source)
    externs, native, production = {}, set(), None
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
                    assert name not in externs or externs[name] == file, f"Ambiguous extern: {name}"
                    externs[name] = file
    assert finished and set(externs) == EXTERNS and production is not None
    dependencies = checked_hashes([cargo_json, basis_path, production])
    for name, file in externs.items():
        expected = basis["externs"][name]
        assert file == expected["path"] and digest(Path(file)) == expected["sha256"], f"Approved extern changed: {name}"
        dependencies[file] = expected["sha256"]
    # Only project-built archives have a recorded build provenance here
    for path in native:
        directory = Path(path.removeprefix("native="))
        if directory.is_relative_to(ROOT / "target"):
            dependencies.update(checked_hashes(sorted(directory.glob("*.a"))))
    binary = output / "candidates-workbench-check"
    assert not binary.exists(), "Use a new evidence directory"
    command = ["rustc", "--edition=2024", "--crate-name=candidates_workbench_check", "-D", "warnings",
               str(output / "source/main.rs"), "-o", str(binary)]
    for directory in sorted({str(Path(file).parent) for file in externs.values()}):
        command.extend(["-L", "dependency=" + directory])
    for path in sorted(native):
        command.extend(["-L", path])
    for name, file in sorted(externs.items()):
        command.extend(["--extern", name + "=" + file])
    env = os.environ.copy()
    env["CARGO_MANIFEST_DIR"] = str(ROOT / "tools/cocobeat-lab")
    started = time.monotonic()
    with (output / "build.log").open("wb") as log:
        result = subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
    report = {
        "command": command, "cwd": str(ROOT), "exit_code": result.returncode, "wall_seconds": time.monotonic() - started,
        "prepared": prepared, "dependency_input_sha256": dependencies,
        "verified_lab_and_catalog_sha256": verified_source,
        "dependency_basis": "Exact approved debug externs and current lab/catalog hashes from supplied successful rustc test record; subsequent C source edits and the current whole repository are not asserted to have been compiled into these artifacts",
        "native_link_paths": sorted(native), "system_native_libraries": "Environment-provided; not independently hashed or accepted by this check",
        "binary": str(binary), "binary_sha256": digest(binary) if binary.exists() else None,
        "build_log_sha256": digest(output / "build.log"),
    }
    save(output / "build.json", report)
    verify_source(output)
    verify_hashes(dependencies)
    verify_hashes(verified_source)
    assert result.returncode == 0, f"Helper compilation failed: {output / 'build.log'}"
    print(f"BUILT {binary}", flush=True)


def run(output):
    build_record = verify_build(output)
    helper = Path(build_record["binary"])
    fixtures = output / "fixtures"
    assert not fixtures.exists(), "Use a new evidence directory"
    command = [str(helper), "--fixture", str(fixtures)]
    started = time.monotonic()
    memory_session = output / ":memory:.ses"
    memory_existed = memory_session.exists()
    with (output / "fixture.log").open("wb") as log:
        try:
            fixture_result = subprocess.run(command, cwd=output, stdout=log, stderr=subprocess.STDOUT, timeout=60)
            fixture_exit, fixture_timeout = fixture_result.returncode, False
        except subprocess.TimeoutExpired:
            fixture_exit, fixture_timeout = None, True
    fixture_record = {"command": command, "cwd": str(output), "exit_code": fixture_exit,
                      "timed_out": fixture_timeout, "wall_seconds": time.monotonic() - started,
                      "cwd_memory_session": {"existed_before": memory_existed,
                                             "created_by_fixture": not memory_existed and memory_session.exists(),
                                             "sha256": digest(memory_session) if memory_session.is_file() else None},
                      "log_sha256": digest(output / "fixture.log"), "generated_sha256": tree(fixtures)}
    save(output / "fixture.json", fixture_record)
    assert fixture_exit == 0 and not fixture_timeout, f"Fixture creation failed: {output / 'fixture.log'}"
    originals = tree(fixtures)
    result = {
        "build": build_record, "fixtures": fixture_record,
        "input_basis": "Constructed candidate reports, synthetic Bevy KeyboardInput through unchanged production capture",
        "render_basis": "Native Bevy Screenshot GPU readback in owned headless gamescope displays",
        "physical_input_audio_human_acceptance": "NOT RUN", "visual_review": "NOT RUN", "cases": [],
    }
    for kind, width, height in [("rich", 1280, 800), ("rich", 640, 480), ("empty", 640, 480)]:
        verify_build(output)
        assert tree(fixtures) == originals, "Source package/report changed"
        case = output / f"case-{kind}-{width}"
        case.mkdir()
        cache = case / "cache"
        cache.mkdir()
        package, candidate_report = fixtures / kind / "package", fixtures / kind / "report.json"
        env = os.environ.copy()
        env.pop("DISPLAY", None)
        env.pop("WAYLAND_DISPLAY", None)
        env.update({"XDG_CACHE_HOME": str(cache), "WINIT_UNIX_BACKEND": "x11", "DISABLE_GAMESCOPE_WSI": "1",
                    "WGPU_BACKEND": "vulkan", "QA_SIZE": str(width), "QA_CAPTURE_DIR": str(case), "QA_REPORT": str(candidate_report)})
        command = ["gamescope", "--backend", "headless", "-W", str(width), "-H", str(height), "-w", str(width), "-h", str(height),
                   "--", str(helper), str(package), str(candidate_report)]
        started, timed_out = time.monotonic(), False
        with tempfile.TemporaryDirectory(prefix="cocobeat-candidates-", dir="/tmp") as runtime:
            env["XDG_RUNTIME_DIR"] = runtime
            with (case / "run.log").open("wb") as log:
                process = subprocess.Popen(command, cwd=case, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    process.wait(timeout=60)
                except subprocess.TimeoutExpired:
                    timed_out = True
                finally:
                    cleanup_signals = stop_group(process)
        events = [json.loads(line[3:]) for line in (case / "run.log").read_text(errors="replace").splitlines() if line.startswith("QA ")]
        item = {
            "kind": kind, "size": [width, height], "command": command, "cwd": str(case), "owned_process_group": process.pid,
            "temporary_runtime_dir": runtime,
            "exit_code": process.returncode, "timed_out": timed_out, "wall_seconds": time.monotonic() - started,
            "cleanup_signals": cleanup_signals, "events": events,
            "run_log_sha256": digest(case / "run.log"),
            "cwd_memory_session": {"created_in_owned_case": (case / ":memory:.ses").exists(),
                                   "sha256": digest(case / ":memory:.ses") if (case / ":memory:.ses").is_file() else None},
            "png_sha256": {path.name: digest(path) for path in sorted(case.glob("*.png"))},
            "input_unchanged": tree(fixtures) == originals,
        }
        result["cases"].append(item)
        result["after_fixture_sha256"] = tree(fixtures)
        save(output / "run.json", result)
        verify_build(output)
        assert item["input_unchanged"], "Source package/report changed"
        assert not timed_out and process.returncode == 0, f"QA process failed: {case / 'run.log'}"
        pngs = RICH_PNGS if kind == "rich" else EMPTY_PNGS
        assert set(item["png_sha256"]) == pngs
        for name in pngs:
            header = (case / name).read_bytes()[:24]
            assert header[:8] == b"\x89PNG\r\n\x1a\n" and struct.unpack(">II", header[16:24]) == (width, height), f"Unexpected native PNG size: {name}"
        finished = [event for event in events if event.get("event") == "finished"]
        assert len(finished) == 1, "Driver did not finish all assertions exactly once"
        final = finished[0]
        expected = {"readonly": True, "dirty": False, "saving": False, "editing": False, "drag": False,
                    "source_chart_anchors": 1, "records_checked": 40 if kind == "rich" else 0,
                    "production_admission": "not_assessed"}
        assert all(final.get(name) == value for name, value in expected.items()), f"Unexpected final state: {final}"
        assert final["message_count"] > 0, "Driver did not inject keyboard messages"
        assert {name + ".png" for name in final["screenshots"]} == pngs
        print(f"AUTOMATED_CAPTURE_PASS {kind} {width}x{height}; visual review remains required", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest="action", required=True)
    for action in ("prepare", "build", "run"):
        command = actions.add_parser(action)
        command.add_argument("output", type=Path)
        if action == "build":
            command.add_argument("cargo_json", type=Path)
            command.add_argument("dependency_basis", type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    owned_output = not output.exists() if args.action == "prepare" else (output / "prepared.json").is_file()
    try:
        if args.action == "prepare":
            prepare(output)
        elif args.action == "build":
            build(output, args.cargo_json.resolve(), args.dependency_basis.resolve())
        else:
            run(output)
    except Exception as error:
        if owned_output and output.is_dir():
            save(output / f"{args.action}-error-{time.time_ns()}.json", {"command": sys.argv, "action": args.action,
                                                                     "error_type": type(error).__name__, "error": str(error)})
        raise
