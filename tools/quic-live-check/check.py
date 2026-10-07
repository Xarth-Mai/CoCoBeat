"""Build and exercise public LiveSession workers without Cargo or source instrumentation"""

import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess


CRATES = ("cocobeat_net", "cocobeat_media", "cocobeat_schema", "cocobeat_core", "cocobeat_replay", "serde_json")
OBJECTS = ("song.audio.ogg", "analysis.bin", "chart.bin", "song.package")
SCENARIOS = ("installed", "receive", "cancel-before-ready", "cancel-after-hit", "missing-armed", "wrong-player", "wrong-epoch", "reenter-before-ready", "reenter-after-hit")
ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def build(cargo_json, output):
    output.mkdir()
    artifacts = {name: set() for name in CRATES}
    for line in cargo_json.read_text().splitlines():
        item = json.loads(line)
        if item.get("reason") != "compiler-artifact":
            continue
        name = item["target"]["name"]
        if name in artifacts:
            artifacts[name].update(Path(filename).resolve() for filename in item["filenames"] if filename.endswith(".rlib"))
    assert all(len(paths) == 1 for paths in artifacts.values()), {name: [str(path) for path in paths] for name, paths in artifacts.items()}
    artifacts = {name: next(iter(paths)) for name, paths in artifacts.items()}
    source = ROOT / "tools/quic-live-check/driver.rs"
    copied = output / "driver.rs"
    copied.write_bytes(source.read_bytes())
    driver = output / "driver"
    dependencies = {path.parent for path in artifacts.values()}
    assert len(dependencies) == 1, "one Cargo graph is required"
    command = ["rustc", "--edition=2024", "--crate-name", "quic_live_probe", str(copied), "-o", str(driver), "-L", f"dependency={next(iter(dependencies))}"]
    for name, path in artifacts.items():
        command.extend(["--extern", f"{name}={path}"])
    result = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=90)
    (output / "rustc.stdout").write_bytes(result.stdout)
    (output / "rustc.stderr").write_bytes(result.stderr)
    paths = list((ROOT / "tools/quic-live-check").glob("*"))
    for crate in CRATES:
        if crate.startswith("cocobeat_"):
            paths.extend((ROOT / "crates" / crate.replace("_", "-") / "src").glob("**/*.rs"))
    record = {"command": command, "exit_code": result.returncode, "cargo_json": str(cargo_json), "cargo_json_sha256": digest(cargo_json),
              "source_sha256": {str(path.relative_to(ROOT)): digest(path) for path in sorted(paths) if path.is_file()},
              "externs": {name: {"path": str(path), "sha256": digest(path)} for name, path in artifacts.items()}}
    if driver.exists():
        record["driver_sha256"] = digest(driver)
    save(output / "build.json", record)
    assert result.returncode == 0, (output / "rustc.stderr").read_text()
    print(json.dumps({"status": "PASS", "driver": str(driver), "build": str(output / "build.json")}))


def run(driver, package, output):
    build_record = json.loads((driver.parent / "build.json").read_text())
    assert digest(driver) == build_record["driver_sha256"], "driver differs from recorded build"
    for path, expected in build_record["source_sha256"].items():
        assert digest(ROOT / path) == expected, f"rebuild after source changed: {path}"
    output.mkdir()
    original = {name: digest(package / name) for name in OBJECTS}
    records = []
    for scenario in SCENARIOS:
        destination = output / scenario
        command = [str(driver), scenario, str(package), str(destination)]
        result = subprocess.run(command, cwd=ROOT, capture_output=True, timeout=44 if scenario.startswith("reenter-") else 22)
        (output / f"{scenario}.stdout").write_bytes(result.stdout)
        (output / f"{scenario}.stderr").write_bytes(result.stderr)
        record = {"scenario": scenario, "command": command, "exit_code": result.returncode}
        records.append(record)
        save(output / "commands.json", records)
        assert result.returncode == 0, f"{scenario}: {(output / f'{scenario}.stderr').read_text()}"
        report = json.loads(result.stdout.decode().splitlines()[-1])
        if scenario.startswith("reenter-"):
            assert report["same_process"] and report["old_files_unchanged"] and report["fresh_invitation"] and report["fresh_epoch"] and report["seq_reset"]
            rounds = []
            for index, expected_status in enumerate(("FAILED", "COMPLETE"), start=1):
                directory = destination / f"round-{index}"
                statuses = [json.loads((directory / side / "status.json").read_text()) for side in ("host", "guest")]
                assert all(status["mode"] == "live" and status["protocol_version"] == 5 and status["status"] == expected_status for status in statuses)
                expected = [93, 27] if index == 2 else [1, 0] if scenario == "reenter-after-hit" else [0, 0]
                assert all(status["facts"] == expected for status in statuses)
                assert report["rounds"][index - 1]["owned_workers_finished"]
                if index == 1:
                    assert all(status["authority_replay_blake3"] is None for status in statuses)
                    if scenario == "reenter-before-ready":
                        assert report["rounds"][0]["started"] == [False, False]
                else:
                    assert statuses[0]["epoch"] != rounds[0][0]["epoch"]
                    assert statuses[0]["epoch"] == statuses[1]["epoch"]
                    assert statuses[0]["event_count"] == statuses[1]["event_count"]
                    assert (directory / "host/authority.replay.json").read_bytes() == (directory / "guest/authority.replay.json").read_bytes()
                    assert all(status["network_timing"]["software_start_lateness_ns"] <= 100_000_000 for status in statuses)
                rounds.append(statuses)
            record["report"] = report
            record["statuses_by_round"] = rounds
            record["evidence_sha256"] = {str(path.relative_to(output)): digest(path) for path in sorted(destination.glob("**/*")) if path.is_file() and path.name != "invite.json"}
            save(output / "commands.json", records)
            continue
        statuses = [json.loads((destination / side / "status.json").read_text()) for side in ("host", "guest")]
        record["report"] = report
        record["statuses"] = statuses
        success = scenario in ("installed", "receive")
        assert all(status["mode"] == "live" and status["protocol_version"] == 5 for status in statuses)
        assert [status["status"] for status in statuses] == (["COMPLETE"] * 2 if success else ["FAILED"] * 2)
        expected = [93, 27] if success else [1, 0] if scenario == "cancel-after-hit" else [0, 0]
        assert all(status["facts"] == expected for status in statuses), scenario
        if success:
            assert all(status["network_timing"]["software_start_lateness_ns"] <= 100_000_000 for status in statuses)
            assert statuses[0]["network_timing"]["host_start_ns"] == statuses[1]["network_timing"]["host_start_ns"]
            assert statuses[0]["event_count"] == statuses[1]["event_count"]
            assert (destination / "host/authority.replay.json").read_bytes() == (destination / "guest/authority.replay.json").read_bytes()
            assert statuses[1]["package_received"] == (scenario == "receive")
        else:
            assert all(status["authority_replay_blake3"] is None for status in statuses)
            if scenario in ("cancel-before-ready", "missing-armed"):
                assert report["started"] == [False, False]
                assert all(status["network_timing"] is None or status["network_timing"]["software_start_observed_ns"] is None for status in statuses)
        if scenario == "receive":
            assert {name: digest(destination / "received" / name) for name in OBJECTS} == original
        assert not list(destination.glob(".cocobeat-package-*")), "owned staging leak"
        record["evidence_sha256"] = {str(path.relative_to(output)): digest(path) for path in sorted(destination.glob("**/*")) if path.is_file() and path.name != "invite.json"}
    assert {name: digest(package / name) for name in OBJECTS} == original
    summary = {"status": "PASS", "transport": "real QUIC loopback, public production LiveSession workers", "commands": records,
               "source_objects_sha256": original, "driver_sha256": digest(driver), "build_sha256": digest(driver.parent / "build.json"),
               "scope": "software network and Replay/core evidence; Armed is a probe acknowledgment, no PCM scheduling, audio device, physical input, native game window or two-machine acceptance"}
    save(output / "summary.json", summary)
    print(json.dumps({"status": "PASS", "scenarios": len(records), "evidence": str(output / "summary.json")}))


def fixture(driver, source, output, repeat):
    output.mkdir()
    assert digest(driver) == json.loads((driver.parent / "build.json").read_text())["driver_sha256"]
    ffmpeg = Path(shutil.which("ffmpeg")).resolve()
    commands = []

    def execute(name, command):
        result = subprocess.run(list(map(str, command)), capture_output=True, timeout=30)
        (output / f"{name}.stdout").write_bytes(result.stdout)
        (output / f"{name}.stderr").write_bytes(result.stderr)
        commands.append({"name": name, "command": list(map(str, command)), "exit_code": result.returncode})
        save(output / "commands.json", commands)
        assert result.returncode == 0, (output / f"{name}.stderr").read_text()
        return result

    execute("ffmpeg-version", [ffmpeg, "-version"])
    pcm = execute("decode-repeat-pcm", [driver, "fixture-pcm", source, output / "source.wav", repeat])
    execute("qa-encode", [ffmpeg, "-nostdin", "-hide_banner", "-loglevel", "error", "-i", output / "source.wav", "-c:a", "libvorbis", "-q:a", "6", "-ar", "48000", "-ac", "2", "-n", output / "canonical.ogg"])
    package = execute("production-package-readback", [driver, "fixture-package", output / "canonical.ogg", output / "package", 4800 * repeat])
    record = {"status": "PASS", "commands": commands, "decoded_pcm": json.loads(pcm.stdout), "package_readback": json.loads(package.stdout),
              "source": str(source), "source_sha256": digest(source), "driver_sha256": digest(driver), "ffmpeg_sha256": digest(ffmpeg),
              "evidence_sha256": {str(path.relative_to(output)): digest(path) for path in sorted(output.glob("**/*")) if path.is_file()},
              "scope": "original synthetic PCM repeated and encoded by external QA libvorbis, strict complete production package readback; not a production encoder admission or MIR quality claim"}
    save(output / "summary.json", record)
    print(json.dumps({"status": "PASS", "package": str(output / "package"), "canonical_frames":4800 * repeat}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    building = sub.add_parser("build", help="select exact rlibs from one completed root Cargo JSON build")
    building.add_argument("cargo_json", type=Path)
    building.add_argument("new_output", type=Path)
    running = sub.add_parser("run", help="run bounded real loopback scenarios using the recorded driver")
    running.add_argument("driver", type=Path)
    running.add_argument("package", type=Path)
    running.add_argument("new_output", type=Path)
    generating = sub.add_parser("fixture", help="repeat original synthetic PCM and use explicit QA libvorbis encoding with production full readback")
    generating.add_argument("driver", type=Path)
    generating.add_argument("source", type=Path)
    generating.add_argument("new_output", type=Path)
    generating.add_argument("--repeat", type=int, choices=(12, 20), default=20)
    args = parser.parse_args()
    if args.action == "build":
        build(args.cargo_json.resolve(), args.new_output.resolve())
    elif args.action == "run":
        run(args.driver.resolve(), args.package.resolve(), args.new_output.resolve())
    else:
        fixture(args.driver.resolve(), args.source.resolve(), args.new_output.resolve(), args.repeat)
