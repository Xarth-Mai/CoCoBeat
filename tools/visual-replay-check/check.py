"""Exercise fixed production watcher binaries with owned synthetic native input"""

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess

OBJECTS = ("song.audio.ogg", "analysis.bin", "chart.bin", "song.package")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def check(game, receipt, lab, package, output, sizes="all"):
    built = json.loads(receipt.read_text())
    assert built["status"] == "PASS" and built["exit_code"] == 0 and built["inputs_unchanged"]
    assert digest(game) == built["binary_sha256"], "Use the immutable game copy matching its build receipt"
    output.mkdir()
    protected = {str(path): digest(path) for path in (game, receipt, lab, Path(__file__).resolve(),
                                                    *(package / name for name in OBJECTS))}
    result = {"status": "RUNNING", "requested_sizes": sizes, "game": str(game), "game_sha256": digest(game),
              "build_receipt": str(receipt), "build_receipt_sha256": digest(receipt), "build_scope": built["scope"],
              "lab": str(lab), "lab_sha256": digest(lab), "input_sha256": protected, "commands": [], "cases": [],
              "scope": "Actual native --watch-replay entry, software KeyboardInput/WindowFocused, acknowledged Kira source cursor and original Replay facts",
              "visual_review": "NOT RUN", "physical_devices": "NOT RUN", "speaker_timing": "NOT RUN"}
    save(output / "summary.json", result)

    def unchanged():
        assert {path: digest(Path(path)) for path in protected} == protected, "Protected input/binary/harness changed"

    def execute(name, arguments, expected=None, binary=game):
        unchanged()
        command = [str(binary), *map(str, arguments)]
        run = subprocess.run(command, capture_output=True, timeout=90)
        stdout, stderr = output / f"{name}.stdout", output / f"{name}.stderr"
        stdout.write_bytes(run.stdout)
        stderr.write_bytes(run.stderr)
        result["commands"].append({"name": name, "command": command, "exit_code": run.returncode,
                                   "stdout_sha256": digest(stdout), "stderr_sha256": digest(stderr), "expected_error": expected})
        save(output / "summary.json", result)
        if expected:
            assert run.returncode != 0 and expected in run.stderr.decode(), run.stderr.decode()
        else:
            assert run.returncode == 0, run.stderr.decode()
        unchanged()
        return run.stdout.decode()

    try:
        stage = json.loads(execute("package-stage", ["inspect-stage", package, "0"], binary=lab))
        end, identity = stage["end_frames"], stage["content_id"]
        assert 96_000 <= end <= 4_800_000, "Native observer QA scope is 2 to 100 seconds"
        epoch = 91
        base = {"format": "CoCoBeat Replay", "version": 2, "content_id": identity, "rules_id": "duo-watermark-v1",
                "build_id": "original-native-watcher-qa", "stage_compiler_version": 1, "epoch": epoch,
                "facts": [{"type": "hit", "epoch": epoch, "player": 1, "seq": 0, "song_time_frames": 48_000},
                          {"type": "watermark", "epoch": epoch, "player": 1, "through_frames": end + 48_000},
                          *({"type": "hit", "epoch": epoch, "player": 2, "seq": seq,
                             "song_time_frames": 48_000 + seq} for seq in range(1000))]}
        replays = {}
        for version in (1, 2):
            for complete in (False, True):
                document = json.loads(json.dumps(base))
                document["stage_compiler_version"] = version
                if complete:
                    document["facts"].append({"type": "watermark", "epoch": epoch, "player": 2,
                                               "through_frames": end + 48_000})
                name = f"stage-{version}-{'full' if complete else 'partial'}"
                replay = output / f"{name}.json"
                save(replay, document)
                protected[str(replay)] = digest(replay)
                replays[version, complete] = replay
                execute(f"{name}-core", ["--package", package, "--replay", replay])
                sample = json.loads(execute(f"{name}-stage", ["inspect-replay-stage", package, replay, end // 2], binary=lab))
                assert sample["compiler_version"] == version
        legacy = json.loads(json.dumps(base))
        legacy.pop("stage_compiler_version")
        legacy["version"] = 1
        path = output / "legacy.json"
        save(path, legacy)
        execute("legacy-core", ["--package", package, "--replay", path])
        execute("legacy-watch", ["--package", package, "--watch-replay", path], "Legacy Replay has no recorded Stage version")
        for name, key, value, reason in (("wrong-content", "content_id", "other-package", "content"),
                                         ("wrong-rules", "rules_id", "other-rules", "rules")):
            document = json.loads(json.dumps(base))
            document[key] = value
            path = output / f"{name}.json"
            save(path, document)
            execute(name, ["--package", package, "--watch-replay", path], reason)
        document = json.loads(json.dumps(base))
        document["facts"][0]["song_time_frames"] = end
        path = output / "invalid-hit.json"
        save(path, document)
        execute("invalid-hit", ["--package", package, "--watch-replay", path], "outside the song timeline")

        document = json.loads(json.dumps(base))
        document["stage_compiler_version"] = 4
        path = output / "unknown-stage.json"
        save(path, document)
        execute("unknown-stage", ["--package", package, "--watch-replay", path], "Invalid Replay version or Stage compiler identity")
        execute("watch-next-round", ["--package", package, "--watch-replay", replays[1, False],
                                     "--next-round", "unused-invite", "unused-output"], "require an invited live round")

        cases = ((1, True, (1280, 800)), (2, True, (1280, 800)),
                 (1, False, (640, 480)), (2, False, (640, 480)))
        for version, complete, size in cases:
            if sizes == "small" and size != (640, 480):
                continue
            unchanged()
            name = f"stage-{version}-{'full' if complete else 'partial'}-{size[0]}x{size[1]}"
            cwd = output / f"{name}-cwd"
            cwd.mkdir()
            observation = output / name
            environment = os.environ.copy()
            environment["XDG_CONFIG_HOME"] = str(cwd / "config")
            environment["COCOBEAT_WATCH_OBSERVATION_DIR"] = str(observation)
            environment["COCOBEAT_WATCH_OBSERVATION_SIZE"] = f"{size[0]}x{size[1]}"
            command = ["gamescope", "--backend", "headless", "--expose-wayland",
                       "-W", str(size[0]), "-H", str(size[1]), "-r", "60", "--", str(game),
                       "--package", str(package), "--watch-replay", str(replays[version, complete])]
            stdout, stderr = output / f"{name}.stdout", output / f"{name}.stderr"
            with stdout.open("wb") as out, stderr.open("wb") as err:
                process = subprocess.Popen(command, cwd=cwd, env=environment, stdout=out, stderr=err, start_new_session=True)
                try:
                    code = process.wait(timeout=165)
                finally:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGTERM)
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait()
            item = {"name": name, "command": command, "cwd": str(cwd), "wrapper_pid": process.pid,
                    "exit_code": code, "stdout_sha256": digest(stdout), "stderr_sha256": digest(stderr)}
            result["cases"].append(item)
            save(output / "summary.json", result)
            assert code == 0, stderr.read_text()
            report = json.loads((observation / "result.json").read_text())
            metadata = json.loads((observation / "metadata.json").read_text())
            assert report["status"] == "PASS" and report["facts_unchanged"]
            assert report["epoch"] == report["original_epoch"] == epoch
            assert metadata["stage_version"] == version
            assert report["finished"]["consumed"] == report["finished"]["total"] == len(json.loads(replays[version, complete].read_text())["facts"])
            assert report["finished"]["source_end"] == end
            assert report["paused"]["song_frame"] == report["paused"]["acknowledged_frame"]
            assert report["paused"]["consumed"] == 1, "Future recorded watermark must hold back later old-time Hits until EOF"
            assert set(report["capture"]) == {"ready", "paused", "finished", "restarted", "returned"}
            assert all(capture == {"Ok": list(size)} for capture in report["capture"].values()), report["capture"]
            assert not (cwd / "replays").exists(), "Read-only watcher wrote history"
            rows = list(csv.DictReader((observation / "frames.csv").open()))
            assert len(rows) <= 40_000
            phases = {row["phase"] for row in rows}
            assert {"Ready", "Starting", "Running", "Pausing", "Paused", "Finished"} <= phases, phases
            assert any(float(row["brand_seconds"]) >= 6.60 and row["controls_enabled"] == "true" for row in rows)
            assert all(row["displayed_frames"] == row["acknowledged_frames"] for row in rows if row["phase"] in ("Running", "Paused"))
            assert all(int(row["consumed"]) <= int(row["total"]) and int(row["epoch"]) == epoch for row in rows)
            assert report["finished"]["events"] >= 0
            item.update({"observation": report, "metadata": metadata,
                         "raw_sha256": {str(p.relative_to(output)): digest(p) for p in sorted(observation.iterdir()) if p.is_file()}})
            unchanged()
            save(output / "summary.json", result)
        result["status"] = "PASS"
    except BaseException as error:
        result.update({"status": "FAIL", "error": f"{type(error).__name__}: {error}"})
        raise
    finally:
        result["inputs_unchanged"] = {path: digest(Path(path)) for path in protected} == protected
        save(output / "summary.json", result)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ("game", "receipt", "lab", "package", "output"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--sizes", choices=("all", "small"), default="all")
    args = parser.parse_args()
    check(*(getattr(args, name).resolve() for name in ("game", "receipt", "lab", "package", "output")), sizes=args.sizes)
