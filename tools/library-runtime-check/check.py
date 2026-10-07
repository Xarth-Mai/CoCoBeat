"""Observe fixed production library entry with owned synthetic keyboard input"""

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import struct
import subprocess
import sys
import time

OBJECTS = ("song.audio.ogg", "analysis.bin", "chart.bin", "song.package")


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def save(path, document):
    path.write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n")


def save_new(path, document):
    with path.open("x") as file:
        json.dump(document, file, ensure_ascii=False, indent=2)
        file.write("\n")


def child(arguments):
    case = Path(arguments[0])
    command = arguments[1:]
    environment = os.environ.copy()
    removed_enable = environment.pop("ENABLE_GAMESCOPE_WSI", None) is not None
    environment["DISABLE_GAMESCOPE_WSI"] = "1"
    names = ("DISPLAY", "WAYLAND_DISPLAY", "XDG_SESSION_TYPE", "WGPU_BACKEND", "XDG_RUNTIME_DIR",
             "PIPEWIRE_RUNTIME_DIR", "XDG_CONFIG_HOME", "ENABLE_GAMESCOPE_WSI", "DISABLE_GAMESCOPE_WSI", "VK_INSTANCE_LAYERS",
             "COCOBEAT_LIBRARY_OBSERVATION_DIR", "COCOBEAT_LIBRARY_OBSERVATION_SIZE",
             "COCOBEAT_LIBRARY_OBSERVATION_LOCALE", "COCOBEAT_LIBRARY_OBSERVATION_SCENARIO")
    with (case / "game.stdout").open("xb") as stdout, (case / "game.stderr").open("xb") as stderr:
        process = subprocess.Popen(command, env=environment, stdout=stdout, stderr=stderr)
        try:
            save_new(case / "game-launch.json", {"command": command, "game_pid": process.pid, "cwd": os.getcwd(),
                                           "launch_monotonic_ns": time.monotonic_ns(),
                                           "removed_enable_gamescope_wsi": removed_enable,
                                           "environment": {name: environment[name] for name in names if name in environment}})
            code = process.wait()
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
    save_new(case / "child-exit.json", {"game_pid": process.pid, "returncode": code,
                                  "exit_monotonic_ns": time.monotonic_ns()})
    raise SystemExit(code if code >= 0 else 128 - code)



def song_state(snapshot):
    return {key: value for key, value in snapshot.items() if key not in ("callback_observation", "source_observation")}


def identity(binary, receipt):
    built = json.loads(receipt.read_text())
    assert built["status"] == "PASS" and built["exit_code"] == 0 and built["inputs_unchanged"]
    assert digest(binary) == built["binary_sha256"], "Use the fixed binary matching its build receipt"
    return {"binary": str(binary), "sha256": digest(binary), "receipt": str(receipt),
            "receipt_sha256": digest(receipt), "scope": built["scope"]}


def check(game, receipt, lab, lab_receipt, package_a, package_b, unknown_rules, output):
    launcher = Path(__file__).resolve()
    binaries = {"game": identity(game, receipt), "lab": identity(lab, lab_receipt)}
    sources = (package_a, package_b, unknown_rules)
    assert all({path.name for path in source.iterdir()} == set(OBJECTS) for source in sources)
    protected = {str(path): digest(path) for path in (game, receipt, lab, lab_receipt, launcher,
                                                    *(source / name for source in sources for name in OBJECTS))}
    output.mkdir()
    result = {"status": "RUNNING", "binaries": binaries, "protected_sha256": protected, "commands": [], "cases": [],
              "scope": "Actual --library menu and bounded background package loading with software input and acknowledged audio cursors",
              "visual_review": "NOT RUN", "physical_devices": "NOT RUN", "speaker_timing": "NOT RUN"}

    def unchanged():
        assert all(digest(Path(path)) == sha for path, sha in protected.items()), "Protected package/binary/harness changed"

    def command(name, arguments):
        unchanged()
        invocation = [str(lab), *map(str, arguments)]
        environment = {key: value for key, value in os.environ.items() if not key.startswith("COCOBEAT_")}
        try:
            run = subprocess.run(invocation, capture_output=True, timeout=90, env=environment)
            out, err, code, timed_out = run.stdout, run.stderr, run.returncode, False
        except subprocess.TimeoutExpired as error:
            out, err, code, timed_out = error.stdout or b"", error.stderr or b"", None, True
        stdout, stderr = output / f"{name}.stdout", output / f"{name}.stderr"
        stdout.write_bytes(out)
        stderr.write_bytes(err)
        result["commands"].append({"name": name, "command": invocation, "exit_code": code, "timed_out": timed_out,
                                   "stdout_sha256": digest(stdout), "stderr_sha256": digest(stderr)})
        save(output / "summary.json", result)
        assert code == 0 and not timed_out, err.decode(errors="replace")
        unchanged()
        return out.decode()

    try:
        stages = [json.loads(command(f"stage-{index}", ["inspect-stage", source, 0]))
                  for index, source in enumerate(sources)]
        assert stages[0]["content_id"] != stages[1]["content_id"], "Use two different package identities"
        assert all(96_000 <= stage["end_frames"] <= 4_800_000 for stage in stages[:2])
        command("unknown-rules-structural", ["verify-package", unknown_rules])
        cases = [("complete", size, locale) for size in ((1280, 800), (640, 480)) for locale in ("zh-CN", "en-US")]
        cases.extend((scenario, (640, 480), "en-US") for scenario in ("close-scan", "close-load"))
        for scenario, size, locale in cases:
            unchanged()
            name = f"{scenario}-{size[0]}x{size[1]}-{locale}"
            case = output / name
            case.mkdir()
            root, cwd, observation = case / "songs", case / "cwd", case / "observation"
            root.mkdir()
            cwd.mkdir()
            for directory, source in (("00-valid-a", package_a), ("01-valid-b", package_b),
                                      ("02-bad-audio", package_a), ("03-extra-file", package_a),
                                      ("04-unknown-rules", unknown_rules)):
                shutil.copytree(source, root / directory)
            audio = root / "02-bad-audio/song.audio.ogg"
            with audio.open("r+b") as file:
                file.truncate(32)
            (root / "03-extra-file/unexpected.txt").write_text("Owned extra-object rejection control\n")
            copied = {str(path): digest(path) for path in root.rglob("*") if path.is_file()}
            environment = {key: value for key, value in os.environ.items() if not key.startswith("COCOBEAT_")}
            environment.pop("ENABLE_GAMESCOPE_WSI", None)
            environment.update({"XDG_CONFIG_HOME": str(cwd / "config"),
                                "DISABLE_GAMESCOPE_WSI": "1",
                                "COCOBEAT_LIBRARY_OBSERVATION_DIR": str(observation),
                                "COCOBEAT_LIBRARY_OBSERVATION_SIZE": f"{size[0]}x{size[1]}",
                                "COCOBEAT_LIBRARY_OBSERVATION_LOCALE": locale,
                                "COCOBEAT_LIBRARY_OBSERVATION_SCENARIO": scenario})
            invocation = ["gamescope", "--backend", "headless", "--expose-wayland", "-W", str(size[0]),
                          "-H", str(size[1]), "-r", "60", "--", sys.executable, str(launcher), "--child", str(case), str(game), "--library", str(root)]
            stdout, stderr = case / "stdout", case / "stderr"
            timed_out = False
            with stdout.open("wb") as out, stderr.open("wb") as err:
                process = subprocess.Popen(invocation, cwd=cwd, env=environment, stdout=out, stderr=err, start_new_session=True)
                try:
                    process.wait(timeout=180)
                except subprocess.TimeoutExpired:
                    timed_out = True
                finally:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGTERM)
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait()
            game_launch = json.loads((case / "game-launch.json").read_text()) if (case / "game-launch.json").exists() else None
            game_exit = json.loads((case / "child-exit.json").read_text()) if (case / "child-exit.json").exists() else None
            item = {"name": name, "command": invocation, "wrapper_pid": process.pid, "wrapper_exit_code": process.returncode, "timed_out": timed_out,
                    "game_launch": game_launch, "game_exit": game_exit,
                    "stdout_sha256": digest(stdout), "stderr_sha256": digest(stderr), "copied_sha256": copied}
            item["game_process_sha256"] = {path.name: digest(path) for path in (case / name for name in ("game-launch.json", "child-exit.json", "game.stdout", "game.stderr")) if path.exists()}
            result["cases"].append(item)
            save(output / "summary.json", result)
            assert process.returncode == 0 and not timed_out, stderr.read_text(errors="replace")
            assert game_launch and game_exit and game_exit["returncode"] == 0, "Actual game failed or lacks its exit record; inspect game.stderr and child-exit.json"
            assert game_exit["game_pid"] == game_launch["game_pid"]
            assert game_launch["command"] == [str(game), "--library", str(root)]
            assert game_launch["cwd"] == str(cwd)
            assert game_launch["environment"]["DISABLE_GAMESCOPE_WSI"] == "1"
            assert "ENABLE_GAMESCOPE_WSI" not in game_launch["environment"]
            item["game_wsi_lines"] = [line for line in (case / "game.stderr").read_text(errors="replace").splitlines() if "[Gamescope WSI]" in line or "error 4" in line.lower()]
            save(output / "summary.json", result)
            assert item["game_wsi_lines"] == [], "Disabled Gamescope WSI still initialized in the actual game"
            report = json.loads((observation / "result.json").read_text())
            metadata = json.loads((observation / "metadata.json").read_text())
            assert report["status"] == "PASS" and report["scenario"] == scenario and report["owned_workers_finished"]
            assert metadata["scenario"] == scenario and metadata["locale"] == locale and metadata["size"] == list(size)
            assert metadata["process_id"] == report["process_id"]
            assert metadata["process_id"] == game_launch["game_pid"]
            assert metadata["disable_gamescope_wsi"] == "1"
            with (observation / "frames.csv").open() as file:
                rows = list(csv.DictReader(file))
            assert 0 < len(rows) <= 40_000 and any(row["controls_enabled"] == "false" for row in rows)
            assert all(digest(Path(path)) == sha for path, sha in copied.items()), "Library package objects changed"
            if scenario == "complete":
                navigation = report["navigation"]
                for name, index, role in (("library-refresh", 6, "Action"), ("library-back", 7, "Action"),
                                           ("library-information", 8, "Information")):
                    assert navigation[name]["selected"] and navigation[name]["index"] == index
                    assert navigation[name]["role"] == role and navigation[name]["text"]
                snapshots = report["snapshots"]
                for key, stage in (("ready_a", stages[0]), ("running_a", stages[0]), ("ready_b", stages[1])):
                    snapshot = snapshots[key]
                    assert snapshot["content_id"] == stage["content_id"]
                    assert snapshot["canonical_frames"] == stage["end_frames"]
                    assert snapshot["package_path"] == str(root / ("01-valid-b" if key == "ready_b" else "00-valid-a"))
                assert snapshots["ready_a"]["phase"] == snapshots["ready_b"]["phase"] == "Ready"
                assert not snapshots["ready_a"]["audio_started"] and not snapshots["ready_b"]["audio_started"]
                assert snapshots["running_a"]["phase"] == "Running" and snapshots["running_a"]["audio_started"]
                assert snapshots["running_a"]["source_position_seconds"] >= 0.5 and snapshots["running_a"]["acknowledged_frame"] > 0
                assert snapshots["running_a"]["epoch"] == snapshots["ready_a"]["epoch"] + 1
                assert snapshots["ready_b"]["epoch"] == snapshots["running_a"]["epoch"]
                played = json.loads(bytes(snapshots["paused_a"]["replay_bytes"]))
                hits = [fact for fact in played["facts"] if fact["type"] == "hit"]
                assert len(hits) == 2 and {hit["player"] for hit in hits} == {1, 2}
                assert all(0 <= hit["song_time_frames"] < stages[0]["end_frames"] for hit in hits)
                assert played["content_id"] == stages[0]["content_id"] and played["epoch"] == snapshots["running_a"]["epoch"]
                saved = list((cwd / "replays").glob("*.json"))
                assert saved, "Original played package history was not saved before switching"
                histories = [json.loads(path.read_text()) for path in saved]
                assert any(history["content_id"] == played["content_id"] and history["epoch"] == played["epoch"]
                           and history["facts"] == played["facts"] for history in histories)
                assert {item["name"] for item in report["rejections"]} == {"bad-audio", "extra-file", "unknown-rules"}
                assert all(item["error"] and song_state(item["before"]) == song_state(item["after"]) for item in report["rejections"])
                assert "Unsupported song ruleset" in json.dumps(next(item for item in report["rejections"] if item["name"] == "unknown-rules")["error"])
                assert song_state(report["cancel"]["before"]) == song_state(report["cancel"]["after"]) and report["cancel"]["worker_finished"]
                assert {"library", "library-refresh", "library-back", "library-information", "loading", "ready-a", "running-a", "ready-b", "rejected", "cancelled"} <= set(report["capture"])
                assert all(capture == {"Ok": list(size)} for capture in report["capture"].values())
                for name in report["capture"]:
                    with (observation / f"{name}.png").open("rb") as image:
                        header = image.read(24)
                    assert header[:8] == b"\x89PNG\r\n\x1a\n" and struct.unpack(">II", header[16:24]) == size
                assert {"Ready", "Starting", "Running", "Pausing", "Paused"} <= {row["phase"] for row in rows}
                assert all(float(row["brand_seconds"]) >= 6.60 for row in rows if row["phase"] == "Running")
            else:
                assert report["close_requested_while_owned"]
                assert isinstance(report["worker_unfinished_at_close"], bool)
            item.update({"report": report, "metadata": metadata, "raw_sha256": {str(path.relative_to(case)): digest(path)
                                                            for path in observation.rglob("*") if path.is_file()}})
            unchanged()
            save(output / "summary.json", result)
        result["status"] = "PASS"
    except BaseException as error:
        result.update({"status": "FAIL", "error": f"{type(error).__name__}: {error}"})
        raise
    finally:
        result["protected_inputs_unchanged"] = all(digest(Path(path)) == sha for path, sha in protected.items())
        save(output / "summary.json", result)


if __name__ == "__main__":
    if sys.argv[1:2] == ["--child"]:
        child(sys.argv[2:])
    parser = argparse.ArgumentParser(description=__doc__)
    names = ("game", "receipt", "lab", "lab-receipt", "package-a", "package-b", "unknown-rules", "output")
    for name in names:
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    check(*(getattr(args, name.replace("-", "_")).resolve() for name in names))
