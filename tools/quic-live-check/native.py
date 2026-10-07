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


def check(game, package, output, compositor):
    output.mkdir()
    invite = output / "invite.json"
    records = []
    processes = []
    streams = []
    try:
        for side in ("host", "guest"):
            work = output / f"{side}-cwd"
            work.mkdir()
            arguments = (["--package", str(package), "--net-host", "127.0.0.1:0", str(invite), str(output / "host-network")]
                         if side == "host" else ["--net-receive", str(invite), str(output / "received"), str(output / "guest-network")])
            wrapper = (["xvfb-run", "-a", "-s", "-screen 0 1280x800x24"] if compositor == "xvfb" else
                       ["gamescope", "--backend", "headless", "--expose-wayland", "-W", "1280", "-H", "800", "-r", "60", "--"])
            command = [*wrapper, str(game), *arguments, "--live-observation", str(output / f"{side}-observation")]
            stdout = (output / f"{side}.stdout").open("wb")
            stderr = (output / f"{side}.stderr").open("wb")
            streams.extend([stdout, stderr])
            process = subprocess.Popen(command, cwd=work, stdout=stdout, stderr=stderr, start_new_session=True)
            processes.append(process)
            records.append({"side": side, "command": command, "cwd": str(work)})
            if side == "host":
                deadline = time.monotonic() + 125
                while not invite.exists():
                    assert process.poll() is None, "host exited before invitation"
                    assert time.monotonic() < deadline, "host invitation timeout"
                    time.sleep(0.05)
        for record, process in zip(records, processes):
            record["exit_code"] = process.wait(timeout=130)
            assert record["exit_code"] == 0, f"{record['side']} failed; inspect stderr and result.json"
        reports = []
        for record in records:
            side = record["side"]
            directory = output / f"{side}-observation"
            result = json.loads((directory / "result.json").read_text())
            status = json.loads((output / f"{side}-network/status.json").read_text())
            assert result["status"] == status["status"] == "COMPLETE"
            assert result["network_started"] and result["local_ended"]
            assert result["synthetic_hits_requested"] == len(result["capture_diagnostics"]) == 3
            assert result["facts"] == sum(status["facts"])
            assert result["events"] == status["event_count"]
            assert result["epoch"] == status["epoch"] and result["content_id"] == status["content_id"]
            assert result["running_capture"]["Ok"] == result["terminal_capture"]["Ok"]
            rows = list(csv.DictReader((directory / "frames.csv").open()))
            assert {"Ready", "Connecting", "Starting", "Running", "Finishing", "Finished"} <= {row["phase"] for row in rows}
            first_running = next(row for row in rows if row["phase"] == "Running")
            assert first_running["brand_complete"] == first_running["network_started"] == "true"
            assert float(first_running["cursor_seconds"]) > 0
            assert result["local_player"] == ("P1" if side == "host" else "P2")
            assert all(capture["player"] == result["local_player"] and capture["consumed_ns"] >= capture["observed_ns"]
                       for capture in result["capture_diagnostics"])
            reports.append({"side": side, "runtime": result, "network": status})
        assert reports[0]["network"]["facts"] == reports[1]["network"]["facts"]
        assert reports[0]["network"]["event_count"] == reports[1]["network"]["event_count"]
        assert (output / "host-network/authority.replay.json").read_bytes() == (output / "guest-network/authority.replay.json").read_bytes()
        original = {name: digest(package / name) for name in OBJECTS}
        assert original == {name: digest(output / "received" / name) for name in OBJECTS}
        summary = {"status": "PASS", "commands": records, "game_sha256": digest(game), "package_objects_sha256": original,
                   "reports": reports, "evidence_sha256": {str(path.relative_to(output)): digest(path) for path in sorted(output.glob("**/*"))
                                                           if path.is_file() and path.name != "invite.json"},
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
    args = parser.parse_args()
    check(args.game.resolve(), args.package.resolve(), args.new_output.resolve(), args.compositor)
