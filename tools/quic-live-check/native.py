"""Observe two native production game processes with synthetic controls on loopback"""

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

OBJECTS = ("song.audio.ogg", "analysis.bin", "chart.bin", "song.package")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def network_directory(output, side, round_number):
    return output / (f"{side}-network" if round_number == 1 else f"{side}-network-{round_number}")


def observation_directory(output, side, round_number, rounds):
    directory = output / f"{side}-observation"
    return directory if rounds == 1 else directory / f"round-{round_number}"


def file_hashes(directory):
    return {str(path.relative_to(directory)): digest(path) for path in sorted(directory.rglob("*")) if path.is_file()}


def check(game, package, output, compositor, rounds=1, scenario="complete"):
    assert rounds >= 1, "round count must be positive"
    assert scenario == "complete" or rounds == 2, "recovery cases require two rounds"
    game_sha256 = digest(game)
    original = {name: digest(package / name) for name in OBJECTS}
    output.mkdir()
    invites = [output / ("invite.json" if number == 1 else f"invite-{number}.json") for number in range(1, rounds + 1)]
    invite = invites[0]
    records = []
    processes = []
    streams = []
    try:
        for side in ("host", "guest"):
            work = output / f"{side}-cwd"
            work.mkdir()
            arguments = (["--package", str(package), "--net-host", "127.0.0.1:0", str(invite), str(output / "host-network")]
                         if side == "host" else ["--net-receive", str(invite), str(output / "received"), str(output / "guest-network")])
            for number in range(2, rounds + 1):
                arguments.extend(["--next-round", str(invites[number - 1]), str(network_directory(output, side, number))])
            wrapper = (["xvfb-run", "-a", "-s", "-screen 0 1280x800x24"] if compositor == "xvfb" else
                       ["gamescope", "--backend", "headless", "--expose-wayland", "-W", "1280", "-H", "800", "-r", "60", "--"])
            command = [*wrapper, str(game), *arguments, "--live-observation", str(output / f"{side}-observation")]
            stdout = (output / f"{side}.stdout").open("wb")
            stderr = (output / f"{side}.stderr").open("wb")
            streams.extend([stdout, stderr])
            environment = os.environ.copy()
            selected = "complete" if scenario == "complete" else f"{scenario}-{side}"
            environment["COCOBEAT_LIVE_OBSERVATION_SCENARIO"] = selected
            assert digest(game) == game_sha256, "game changed between launches"
            process = subprocess.Popen(command, cwd=work, stdout=stdout, stderr=stderr, start_new_session=True, env=environment)
            processes.append(process)
            records.append({"side": side, "command": command, "cwd": str(work), "wrapper_pid": process.pid, "qa_scenario": selected})
            if side == "host":
                deadline = time.monotonic() + 125
                while not invite.exists():
                    assert process.poll() is None, "host exited before invitation"
                    assert time.monotonic() < deadline, "host invitation timeout"
                    time.sleep(0.05)
        preserved = {}
        preserved_local = {}
        deadline = time.monotonic() + 130 * rounds
        while True:
            codes = [process.poll() for process in processes]
            for side in ("host", "guest"):
                for number in range(1, rounds + 1):
                    key = (side, number)
                    directory = observation_directory(output, side, number, rounds)
                    network = network_directory(output, side, number)
                    if key not in preserved and (directory / "result.json").exists() and (network / "status.json").exists():
                        try:
                            status = json.loads((network / "status.json").read_text())
                            result = json.loads((directory / "result.json").read_text())
                        except json.JSONDecodeError:
                            continue
                        expected = "FAILED" if scenario != "complete" and number == 1 else "COMPLETE"
                        if status["status"] == result["status"] == expected:
                            preserved[key] = file_hashes(network)
                            replay_dir = output / f"{side}-cwd" / "replays"
                            preserved_local[key] = {str(path.relative_to(output)): digest(path) for path in replay_dir.glob("*.json")
                                                    if json.loads(path.read_text())["epoch"] == result["epoch"]}
            assert all(code in (None, 0) for code in codes), "native process failed; inspect stderr and round results"
            if all(code is not None for code in codes):
                break
            assert time.monotonic() < deadline, "native rounds exceeded observation deadline"
            time.sleep(0.05)
        for record, process in zip(records, processes):
            record["exit_code"] = process.returncode
        assert len(preserved) == 2 * rounds, "each expected terminal round must be observed before process exit"
        all_reports = []
        process_ids = {"host": set(), "guest": set()}
        epochs = []
        certificates = []
        for number in range(1, rounds + 1):
            reports = []
            for record in records:
                side = record["side"]
                directory = observation_directory(output, side, number, rounds)
                network = network_directory(output, side, number)
                result = json.loads((directory / "result.json").read_text())
                status = json.loads((network / "status.json").read_text())
                assert file_hashes(network) == preserved[(side, number)], "earlier round evidence changed"
                expected = "FAILED" if scenario != "complete" and number == 1 else "COMPLETE"
                assert status["mode"] == "live" and status["protocol_version"] == 5
                assert result["status"] == status["status"] == expected
                assert result["scenario"] == scenario and result["owned_workers_finished"]
                process_ids[side].add(result["process_id"])
                assert result["terminal_capture"]["Ok"] == [1280, 800]
                for path, sha256 in preserved_local[(side, number)].items():
                    assert digest(output / path) == sha256, "old local Replay changed"
                for replay_name in ("live.replay.json", "authority.replay.json"):
                    replay_path = network / replay_name
                    if replay_path.exists():
                        replay = json.loads(replay_path.read_text())
                        assert replay["version"] == 2 and replay["stage_compiler_version"] == 2
                        assert replay["epoch"] == status["epoch"]
                rows = list(csv.DictReader((directory / "frames.csv").open()))
                if expected == "FAILED":
                    assert result["phase"] == "Fault" and result["error"] and status["error"]
                    assert result["local_worker_cancel_requested"] == (side == "host")
                    assert "Fault" in {row["phase"] for row in rows}
                    assert not result["local_ended"] and status.get("authority_replay_blake3") is None
                    assert not (network / "authority.replay.json").exists()
                    if scenario == "reenter-before-ready":
                        assert not result["network_started"] and result["synthetic_hits_requested"] == 0
                        assert status["facts"] == [0, 0] and not result["capture_diagnostics"]
                        assert "Running" not in {row["phase"] for row in rows}
                    else:
                        assert result["network_started"] and result["capture_diagnostics"]
                        assert result["running_capture"]["Ok"] == [1280, 800]
                        assert sum(status["facts"]) > 0
                        assert (network / "live.replay.json").exists()
                        assert result["epoch"] == status["epoch"]
                    prefix_path = network / "live.replay.json"
                    if prefix_path.exists():
                        prefix = json.loads(prefix_path.read_text())
                        assert all(fact["through_frames"] < result["canonical_frames"] for fact in prefix["facts"] if fact["type"] == "watermark"), "failed prefix must not invent an EOF watermark"
                    reports.append({"side": side, "runtime": result, "network": status})
                    continue
                assert result["network_started"] and result["local_ended"]
                assert result["synthetic_hits_requested"] == len(result["capture_diagnostics"]) == 3
                assert result["facts"] == sum(status["facts"])
                assert result["events"] == status["event_count"]
                assert result["epoch"] == status["epoch"] and result["content_id"] == status["content_id"]
                assert result["running_capture"]["Ok"] == result["terminal_capture"]["Ok"] == [1280, 800]
                phases = {"Connecting", "Starting", "Running", "Finishing", "Finished"}
                if number == 1:
                    phases.add("Ready")
                assert phases <= {row["phase"] for row in rows}
                first_running = next(row for row in rows if row["phase"] == "Running")
                assert first_running["brand_complete"] == first_running["network_started"] == "true"
                assert float(first_running["cursor_seconds"]) > 0
                assert result["local_player"] == ("P1" if side == "host" else "P2")
                assert all(capture["player"] == result["local_player"] and capture["consumed_ns"] >= capture["observed_ns"]
                           for capture in result["capture_diagnostics"])
                assert [capture["seq"] for capture in result["capture_diagnostics"]] == [0, 1, 2]
                authority = json.loads((network / "authority.replay.json").read_text())
                assert all(fact["epoch"] == result["epoch"] for fact in authority["facts"])
                reports.append({"side": side, "runtime": result, "network": status})
            if scenario != "complete" and number == 1:
                # An unprepared receiver has no initialized epoch/content session yet
                if scenario == "reenter-after-hit":
                    assert reports[0]["network"]["epoch"] == reports[1]["network"]["epoch"]
                epochs.append(reports[0]["network"]["epoch"])
                certificates.append(reports[0]["network"]["cert_blake3"])
                all_reports.append(reports)
                continue
            assert reports[0]["network"]["facts"] == reports[1]["network"]["facts"]
            assert reports[0]["network"]["event_count"] == reports[1]["network"]["event_count"]
            assert reports[0]["network"]["epoch"] == reports[1]["network"]["epoch"]
            assert (network_directory(output, "host", number) / "authority.replay.json").read_bytes() == (network_directory(output, "guest", number) / "authority.replay.json").read_bytes()
            assert reports[1]["network"]["package_received"] == (number == 1 or (scenario == "reenter-before-ready" and number == 2))
            epochs.append(reports[0]["network"]["epoch"])
            certificates.append(reports[0]["network"]["cert_blake3"])
            all_reports.append(reports)
        assert len(set(epochs)) == len(set(certificates)) == rounds, "new rounds must use fresh invitation and epoch"
        assert {name: digest(package / name) for name in OBJECTS} == original
        assert digest(game) == game_sha256
        assert all(len(ids) == 1 for ids in process_ids.values()) and len(set().union(*process_ids.values())) == 2
        for side in ("host", "guest"):
            if rounds > 1:
                aggregate = json.loads((output / f"{side}-observation" / "summary.json").read_text())
                assert aggregate["status"] == ("COMPLETE" if scenario == "complete" else "RECOVERED")
            local = list((output / f"{side}-cwd" / "replays").glob("*.json"))
            assert local, "successful round must save local Replay"
            for path in local:
                replay = json.loads(path.read_text())
                assert replay["version"] == 2 and replay["stage_compiler_version"] == 2
                assert replay["epoch"] in epochs
        assert original == {name: digest(output / "received" / name) for name in OBJECTS}
        summary = {"status": "PASS", "commands": records, "game_sha256": game_sha256, "scenario": scenario, "native_process_ids": {side: next(iter(ids)) for side, ids in process_ids.items()}, "package_objects_sha256": original,
                   "reports": all_reports[0], "round_count": rounds, "rounds": all_reports,
                   "preserved_network_sha256": {f"{side}/round-{number}": hashes for (side, number), hashes in preserved.items()},
                   "preserved_local_replay_sha256": {f"{side}/round-{number}": hashes for (side, number), hashes in preserved_local.items()},
                   "evidence_sha256": {str(path.relative_to(output)): digest(path) for path in sorted(output.glob("**/*"))
                                       if path.is_file() and path not in invites},
                   "scope": f"two native Linux game processes, separate {compositor} displays, actual Kira source cursors, synthetic controls, real loopback; physical input, speakers, two machines and human acceptance NOT RUN"}
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps({"status": "PASS", "evidence": str(output / "summary.json")}))
    finally:
        for record, process in zip(records, processes):
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                process.wait(timeout=10)
            record["exit_code"] = process.returncode
        for stream in streams:
            stream.close()
        (output / "commands.json").write_text(json.dumps(records, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("game", type=Path)
    parser.add_argument("package", type=Path)
    parser.add_argument("new_output", type=Path)
    parser.add_argument("--compositor", choices=("xvfb", "gamescope"), default="xvfb")
    parser.add_argument("--rounds", type=int, default=1)
    parser.add_argument("--scenario", choices=("complete", "reenter-before-ready", "reenter-after-hit"), default="complete")
    args = parser.parse_args()
    check(args.game.resolve(), args.package.resolve(), args.new_output.resolve(), args.compositor, args.rounds, args.scenario)
