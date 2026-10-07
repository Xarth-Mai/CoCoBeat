"""Exercise fixed production lab imports and native no-audio package loading"""

import argparse
import importlib.util
import json
import math
import os
from pathlib import Path
import re
import struct
import subprocess
import sys
import tempfile
import time
import wave

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[2]
COMMON = ROOT / "tools/replay-workbench-check/check.py"
spec = importlib.util.spec_from_file_location("replay_check", COMMON)
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)
digest, save, stop_group = common.digest, common.save, common.stop_group
OBJECTS = ("song.audio.ogg", "analysis.bin", "chart.bin", "song.package")
PROFILE = "aotuv-lancer-vorbis-q10-v1"
DEV_SHA256 = "3390dd080cb536fd4a598ea933618dd99b4bf697c0b2874ff5cb0e220feb09e8"


def check(lab, lab_receipt, game, game_receipt, dev_source, output):
    def identity(binary, receipt):
        record = json.loads(receipt.read_text())
        assert record["exit_code"] == 0 and record["status"] == "PASS" and record["inputs_unchanged"], record
        assert digest(binary) == record["binary_sha256"], "Use the fixed binary from this receipt"
        return {"binary": str(binary), "sha256": digest(binary), "receipt": str(receipt),
                "receipt_sha256": digest(receipt), "build_scope": record["scope"]}

    binaries = {"lab": identity(lab, lab_receipt), "game": identity(game, game_receipt)}
    assert digest(dev_source) == DEV_SHA256, "Expected unchanged original 64-second development WAV"
    authoring = ROOT / "assets/dev/vertical_slice/authoring.json"
    protected = {str(path): digest(path) for path in (lab, game, lab_receipt, game_receipt, dev_source, authoring, COMMON, Path(__file__).resolve())}
    output.mkdir()
    result = {"status": "RUNNING", "binaries": binaries, "input_sha256": protected, "commands": [], "packages": [],
              "scope": "Production source import CLI, strict PCM readback and native fixed-state package loader/Stage sampling; no audio device or physical/human acceptance",
              "visual_review": "NOT RUN"}
    save(output / "summary.json", result)

    def unchanged():
        assert {path: digest(Path(path)) for path in protected} == protected, "Protected source/binary/receipt/harness changed"

    def execute(name, arguments, failure=None):
        unchanged()
        command = [str(lab), *map(str, arguments)]
        started = time.monotonic()
        run = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=90)
        stdout, stderr = output / f"{name}.stdout", output / f"{name}.stderr"
        stdout.write_bytes(run.stdout)
        stderr.write_bytes(run.stderr)
        item = {"name": name, "command": command, "exit_code": run.returncode, "wall_seconds": time.monotonic() - started,
                "stdout_sha256": digest(stdout), "stderr_sha256": digest(stderr), "expected_error": failure}
        result["commands"].append(item)
        save(output / "summary.json", result)
        unchanged()
        if failure is None:
            assert run.returncode == 0, stderr.read_text()
        else:
            assert run.returncode != 0 and failure in stderr.read_text(), stderr.read_text()
        return run.stdout.decode()

    short_source = output / "short-44100.wav"
    with wave.open(str(short_source), "wb") as wav:
        wav.setparams((2, 2, 44100, 4410, "NONE", "not compressed"))
        wav.writeframes(b"".join(struct.pack("<hh", int(math.sin(math.tau * 440 * i / 44100) * 8192),
                                             int(math.sin(math.tau * 1000 * i / 44100) * -16384)) for i in range(4410)))
    short_authoring = output / "short-authoring.json"
    document = {"schema_version": 1, "song_id": "original-short-import-check", "ruleset_id": "duo-watermark-v1",
                "source_note": "Original stereo tones for source import QA; hand-authored final 48 kHz coordinates",
                "anchors": [{"id": 1, "frame": 2400}],
                "sections": [{"id": 1, "start_frame": 0, "end_frame": 4800, "label": "short"}]}
    save(short_authoring, document)
    protected.update({str(path): digest(path) for path in (short_source, short_authoring)})
    stages = {}
    try:
        for name, source, authored, rate, source_frames, frames, sample in (
            ("short", short_source, short_authoring, 44100, 4410, 4800, 2400),
            ("original-64s", dev_source, authoring, 48000, 3072000, 3072000, 1344000),
        ):
            package = output / f"{name}-package"
            text = execute(f"{name}-import", ["import-authored-package", source, authored, package])
            assert f"{frames} frames" in text, text
            assert {path.name for path in package.iterdir()} == set(OBJECTS)
            hashes = {obj: digest(package / obj) for obj in OBJECTS}
            protected.update({str(package / obj): sha for obj, sha in hashes.items()})
            verify = execute(f"{name}-verify", ["verify-package", package])
            assert f"{frames} frames" in verify, verify
            match = re.search(rb"; source import: (\{[^}]+\})", (package / "analysis.bin").read_bytes())
            assert match, "Missing source provenance in validated analysis diagnostics"
            provenance = json.loads(match.group(1))
            assert provenance["source_bytes"] == source.stat().st_size
            assert provenance["source_sample_rate"] == rate and provenance["source_frames"] == source_frames
            assert provenance["canonical_frames"] == frames and provenance["encoder_profile"] == PROFILE
            assert re.fullmatch("[0-9a-f]{64}", provenance["source_blake3"])
            assert PROFILE.encode() in (package / "song.package").read_bytes()
            pcm = output / f"{name}.f32le"
            execute(f"{name}-readback", ["readback-canonical", package / "song.audio.ogg", frames, pcm])
            assert pcm.stat().st_size == frames * 8
            with pcm.open("rb") as file:
                while block := file.read(65536):
                    assert all(math.isfinite(value[0]) for value in struct.iter_unpack("<f", block))
            stage = json.loads(execute(f"{name}-stage", ["inspect-stage", package, sample]))
            end = json.loads(execute(f"{name}-end", ["inspect-stage", package, frames]))
            assert stage["end_frames"] == frames and stage["frame"] == sample and not stage["at_end"]
            assert end["end_frames"] == frames and end["frame"] == frames and end["at_end"]
            assert stage["compiler_version"] == 2 and end["compiler_version"] == 2
            assert {obj: digest(package / obj) for obj in OBJECTS} == hashes
            stages[name] = (package, sample, stage, frames, hashes)
            result["packages"].append({"name": name, "path": str(package), "source_sha256": digest(source), "authoring_sha256": digest(authored),
                                       "provenance": provenance, "objects_sha256": hashes, "pcm_sha256": digest(pcm), "stage": stage, "end": end})
            save(output / "summary.json", result)

        package = stages["short"][0]
        execute("existing-package", ["import-authored-package", short_source, short_authoring, package], "already exists")
        existing = output / "existing-file"
        existing.write_bytes(b"User file must remain unchanged")
        before = digest(existing)
        execute("existing-file", ["import-authored-package", short_source, short_authoring, existing], "already exists")
        assert digest(existing) == before
        symlink = output / "existing-link"
        symlink.symlink_to(existing)
        execute("existing-link", ["import-authored-package", short_source, short_authoring, symlink], "already exists")
        assert symlink.is_symlink() and symlink.resolve() == existing and digest(existing) == before
        bad_authoring = output / "out-of-range.json"
        invalid = json.loads(json.dumps(document))
        invalid["anchors"][0]["frame"] = 4801
        save(bad_authoring, invalid)
        rejected = output / "rejected"
        execute("out-of-range", ["import-authored-package", short_source, bad_authoring, rejected], "outside")
        assert not rejected.exists()
        bad_source = output / "bad-source"
        bad_source.write_bytes(b"Not a media file")
        before = digest(bad_source)
        execute("bad-source", ["import-authored-package", bad_source, short_authoring, rejected], "invalid audio source")
        assert not rejected.exists() and digest(bad_source) == before
        assert not list(output.glob(".cocobeat-*")), "Returned-error staging leak"
        result["failure_controls"] = 5

        for name, (package, frame, stage, frames, hashes) in stages.items():
            unchanged()
            case = output / f"{name}-native"
            case.mkdir()
            png = case / "scene.png"
            env = os.environ.copy()
            env.pop("DISPLAY", None)
            env.pop("WAYLAND_DISPLAY", None)
            env.update({"WINIT_UNIX_BACKEND": "x11", "DISABLE_GAMESCOPE_WSI": "1", "WGPU_BACKEND": "vulkan"})
            command = ["gamescope", "--backend", "headless", "-W", "1280", "-H", "720", "-w", "1280", "-h", "720", "--", str(game),
                       "--package", str(package), "--section-smoke", str(frame), "en-US", "high", "1280", "720", "1", str(png)]
            with tempfile.TemporaryDirectory(prefix="cocobeat-source-", dir="/tmp") as runtime:
                env["XDG_RUNTIME_DIR"] = runtime
                started, timed_out = time.monotonic(), False
                with (case / "run.log").open("wb") as log:
                    process = subprocess.Popen(command, cwd=case, env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
                    try:
                        process.wait(timeout=60)
                    except subprocess.TimeoutExpired:
                        timed_out = True
                    finally:
                        signals = stop_group(process)
            log = (case / "run.log").read_text(errors="replace")
            events = [json.loads(line.split("CONTENT_SAMPLE ", 1)[1]) for line in log.splitlines() if "CONTENT_SAMPLE " in line]
            item = {"name": name, "command": command, "exit_code": process.returncode, "timed_out": timed_out, "cleanup_signals": signals,
                    "owned_process_group": process.pid, "wall_seconds": time.monotonic() - started,
                    "run_log_sha256": digest(case / "run.log"), "events": events}
            result.setdefault("native", []).append(item)
            save(output / "summary.json", result)
            assert process.returncode == 0 and not timed_out, log[-4000:]
            assert len(events) == 1 and events[0]["duration_frames"] == frames
            event = events[0]
            assert event["content_id"] == stage["content_id"]
            assert event["stage"] == {key: value for key, value in stage.items() if key not in ("content_id", "end_frames")}
            header = png.read_bytes()[:24]
            assert header[:8] == b"\x89PNG\r\n\x1a\n" and struct.unpack(">II", header[16:24]) == (1280, 720)
            item["png_sha256"] = digest(png)
            assert {obj: digest(package / obj) for obj in OBJECTS} == hashes
        unchanged()
        assert not list(output.glob(".cocobeat-*"))
        result["status"] = "PASS"
    except BaseException as error:
        result["status"] = "FAIL"
        result["failure"] = repr(error)
        raise
    finally:
        result["inputs_after_sha256"] = {path: digest(Path(path)) for path in protected}
        result["objects_after_sha256"] = {name: {obj: digest(package / obj) for obj in OBJECTS} for name, (package, *_) in stages.items()}
        save(output / "summary.json", result)
    print(json.dumps({"status": result["status"], "evidence": str(output / "summary.json")}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("lab", "lab-receipt", "game", "game-receipt", "dev-source", "output"):
        parser.add_argument(f"--{name}", type=lambda value: Path(value).resolve(), required=True)
    args = parser.parse_args()
    check(args.lab, args.lab_receipt, args.game, args.game_receipt, args.dev_source, args.output)
