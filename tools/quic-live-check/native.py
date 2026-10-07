"""Observe two native production game processes with synthetic controls on loopback"""

import argparse
import csv
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
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


def check(game, package, output, compositor, rounds=1, scenario="complete", receipt=None):
    assert rounds >= 1, "round count must be positive"
    same_epoch = scenario == "same-epoch-active"
    assert not same_epoch or rounds == 1, "same epoch uses one original round"
    assert not scenario.startswith("reenter-") or rounds == 2, "reentry cases require two rounds"
    supervisor = Path(__file__).resolve().parents[1] / "library-runtime-check/check.py"
    protected = {str(path): digest(path) for path in (Path(__file__).resolve(), supervisor)}
    if same_epoch:
        assert receipt is not None, "same epoch requires a fixed successful build receipt"
    if receipt is not None:
        built = json.loads(receipt.read_text())
        assert built["status"] == "PASS" and built["exit_code"] == 0 and built["inputs_unchanged"]
        assert built["binary_sha256"] == digest(game)
        protected[str(receipt)] = digest(receipt)
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
            process_dir = output / f"{side}-process"
            process_dir.mkdir()
            command = [*wrapper, sys.executable, str(supervisor), "--child", str(process_dir), str(game), *arguments, "--live-observation", str(output / f"{side}-observation")]
            stdout = (output / f"{side}.stdout").open("wb")
            stderr = (output / f"{side}.stderr").open("wb")
            streams.extend([stdout, stderr])
            environment = {key: value for key, value in os.environ.items() if not key.startswith("COCOBEAT_")}
            environment.pop("ENABLE_GAMESCOPE_WSI", None)
            environment["DISABLE_GAMESCOPE_WSI"] = "1"
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
        preserved_recovery = {}
        pending_recovery = {}
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
                        expected = "FAILED" if scenario.startswith("reenter-") and number == 1 else "COMPLETE"
                        if status["status"] == result["status"] == expected:
                            preserved[key] = file_hashes(network)
                            replay_dir = output / f"{side}-cwd" / "replays"
                            preserved_local[key] = {str(path.relative_to(output)): digest(path) for path in replay_dir.glob("*.json")
                                                    if json.loads(path.read_text())["epoch"] == result["epoch"]}
            if same_epoch:
                for side in ("host", "guest"):
                    directory = network_directory(output, side, 1) / "recovery-1"
                    names = ("worker-prefix.replay.json", "gui-prefix.replay.json", "metadata.json")
                    if side not in preserved_recovery and all((directory / name).is_file() for name in names):
                        try:
                            raw = {name: (directory / name).read_bytes() for name in names}
                            for value in raw.values():
                                json.loads(value)
                        except json.JSONDecodeError:
                            continue
                        hashes = {name: hashlib.sha256(value).hexdigest() for name, value in raw.items()}
                        if pending_recovery.get(side) == hashes:
                            preserved_recovery[side] = hashes
                        pending_recovery[side] = hashes
            assert all(code in (None, 0) for code in codes), "native process failed; inspect stderr and round results"
            if all(code is not None for code in codes):
                break
            assert time.monotonic() < deadline, "native rounds exceeded observation deadline"
            time.sleep(0.05)
        for record, process in zip(records, processes):
            record["exit_code"] = process.returncode
            process_dir = output / f"{record['side']}-process"
            launch = json.loads((process_dir / "game-launch.json").read_text())
            actual_exit = json.loads((process_dir / "child-exit.json").read_text())
            assert actual_exit["game_pid"] == launch["game_pid"] and actual_exit["returncode"] == 0
            assert launch["environment"]["DISABLE_GAMESCOPE_WSI"] == "1"
            assert not any("[Gamescope WSI]" in line or "error 4" in line.lower() for line in (process_dir / "game.stderr").read_text(errors="replace").splitlines())
            record["actual_game"] = actual_exit
        assert len(records) == 2 and {record["side"] for record in records} == {"host", "guest"}
        assert all(record["wrapper_pid"] > 0 and record["exit_code"] == 0
                   and record["actual_game"]["game_pid"] > 0 and record["actual_game"]["returncode"] == 0
                   and record["command"] and record["cwd"] for record in records)
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
                expected = "FAILED" if scenario.startswith("reenter-") and number == 1 else "COMPLETE"
                assert status["mode"] == "live" and status["protocol_version"] == 6
                assert result["status"] == status["status"] == expected
                assert result["scenario"] == scenario and result["owned_workers_finished"]
                process_ids[side].add(result["process_id"])
                assert result["process_id"] == record["actual_game"]["game_pid"]
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
                if same_epoch:
                    recovery = result["same_epoch"]
                    assert recovery["request_sent"] == (side == "host") and recovery["recovering_seen"] and recovery["ready_after_gate"]
                    recovering = [row for row in rows if row["phase"] == "Recovering"]
                    assert recovering and all(row["online_recovering"] == "true" and int(row["synthetic_hits"]) == 2 for row in recovering)
                    assert recovery["probe_dropped_before_ready"]
                    assert int(recovering[0]["monotonic_ns"]) <= recovery["probe_requested_ns"] <= int(recovering[-1]["monotonic_ns"])
                    assert recovery["probe_requested_ns"] < recovery["probe_cleared_observed_ns"] < result["capture_diagnostics"][2]["observed_ns"]
                    recovery_stages = [stage for stage in recovery["records"] if stage["phase"] == "Recovering"]
                    assert recovery_stages and all(stage["local_hits"] == 2 for stage in recovery_stages)
                    local_player = 1 if side == "host" else 2
                    assert all(sum(fact["type"] == "hit" and fact["player"] == local_player
                                   for fact in json.loads(bytes(stage["facts"]))["facts"]) == 2
                               for stage in recovery_stages), "performing Hit accepted during recovery"
                    assert result["capture_diagnostics"][2]["observed_ns"] > int(recovering[-1]["monotonic_ns"])
                    publications = [row for row in rows if row["source_id"] and row["phase"] in ("Running", "Recovering")]
                    assert len({row["source_id"] for row in publications}) == len({row["source_generation"] for row in publications}) == 1
                    assert all(int(a["source_sequence"]) <= int(b["source_sequence"]) for a, b in zip(publications, publications[1:]))
                    assert side in preserved_recovery
                    directory = network / "recovery-1"
                    assert file_hashes(directory) == preserved_recovery[side]
                    metadata = json.loads((directory / "metadata.json").read_text())
                    assert metadata["epoch"] == result["epoch"] and metadata["attempt"] == 1 and metadata["paused_frame"] > 0
                    expected_causes = ({"explicit local connection maintenance"} if side == "host" else
                                       {"authenticated peer requested connection maintenance", "authenticated peer continuation"})
                    assert metadata["cause"] in expected_causes, "unexpected recovery cause"
                    assert str(metadata["source_id"]) == publications[0]["source_id"]
                    assert str(metadata["source_generation"]) == publications[0]["source_generation"]
                    assert not (network / "recovery-2").exists()
                authority = json.loads((network / "authority.replay.json").read_text())
                assert all(fact["epoch"] == result["epoch"] for fact in authority["facts"])
                if same_epoch:
                    for player in (1, 2):
                        assert [fact["seq"] for fact in authority["facts"]
                                if fact["type"] == "hit" and fact["player"] == player] == [0, 1, 2]
                    stages = result["same_epoch"]["records"]
                    final_gui = json.loads(bytes(stages[-1]["facts"]))
                    for name in ("worker-prefix.replay.json", "gui-prefix.replay.json"):
                        prefix = json.loads((network / "recovery-1" / name).read_text())
                        assert prefix["epoch"] == authority["epoch"] and prefix["content_id"] == authority["content_id"]
                        if name == "gui-prefix.replay.json":
                            assert final_gui["facts"][:len(prefix["facts"])] == prefix["facts"], "original mixed GUI arrival order changed"
                        for player in (1, 2):
                            old = [fact for fact in prefix["facts"] if fact["player"] == player]
                            full = [fact for fact in authority["facts"] if fact["player"] == player]
                            assert full[:len(old)] == old, "original owner Hit/Watermark order changed"
                    frozen = next(record for record in stages if record["phase"] == "Recovering")
                    prefix = json.loads(bytes(frozen["facts"]))["facts"]
                    for record in stages[stages.index(frozen):]:
                        facts = json.loads(bytes(record["facts"]))["facts"]
                        assert facts[:len(prefix)] == prefix, "original ordered GUI prefix changed"

                reports.append({"side": side, "runtime": result, "network": status})
            if scenario.startswith("reenter-") and number == 1:
                # An unprepared receiver has no initialized epoch/content session yet
                if scenario == "reenter-after-hit":
                    assert reports[0]["network"]["epoch"] == reports[1]["network"]["epoch"]
                epochs.append(reports[0]["network"]["epoch"])
                certificates.append(reports[0]["network"]["cert_blake3"])
                all_reports.append(reports)
                continue
            if same_epoch:
                assert reports[0]["runtime"]["core_events"] == reports[1]["runtime"]["core_events"]
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
        assert all(digest(Path(path)) == sha for path, sha in protected.items()), "build receipt/harness changed"
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
                   "harness_and_receipt_sha256": protected, "preserved_recovery_sha256": preserved_recovery,
                   "recovery_assertions": ({"gui-prefix.replay.json": "complete ordered GUI prefix of the same process final GUI Replay; observer also checks full original GUI prefix on every frame",
                                            "worker-prefix.replay.json": "accepted worker original per-player Hit/Watermark prefix of final canonical authority",
                                            "cross_process": "identical canonical authority bytes, events and each player facts; process GUI arrival orders need not match",
                                            "snapshot_preservation": "all three recovery-1 files hash-stable after two consecutive live reads and at final exit"} if same_epoch else None),
                   "preserved_network_sha256": {f"{side}/round-{number}": hashes for (side, number), hashes in preserved.items()},
                   "preserved_local_replay_sha256": {f"{side}/round-{number}": hashes for (side, number), hashes in preserved_local.items()},
                   "evidence_sha256": {str(path.relative_to(output)): digest(path) for path in sorted(output.glob("**/*"))
                                       if path.is_file() and path not in invites},
                   "scope": f"two native Linux game processes, separate {compositor} displays, actual Kira source cursors, synthetic controls, real loopback; physical input, speakers, two machines and human acceptance NOT RUN"}
        (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
        print(json.dumps({"status": "PASS", "evidence": str(output / "summary.json")}))
    except BaseException as error:
        (output / "failure.json").write_text(json.dumps({"status": "FAIL", "error": f"{type(error).__name__}: {error}", "scope": "Owned native software observation; original raw logs retained"}, indent=2) + "\n")
        raise
    finally:
        for record, process in zip(records, processes):
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
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
    parser.add_argument("--scenario", choices=("complete", "reenter-before-ready", "reenter-after-hit", "same-epoch-active"), default="complete")
    parser.add_argument("--receipt", type=Path)
    args = parser.parse_args()
    check(args.game.resolve(), args.package.resolve(), args.new_output.resolve(), args.compositor, args.rounds, args.scenario, args.receipt.resolve() if args.receipt else None)
