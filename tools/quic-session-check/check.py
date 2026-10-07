"""Check real loopback QUIC sessions using the built lab and an owned new directory"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import time


OBJECTS = ("song.audio.ogg", "analysis.bin", "chart.bin", "song.package")


def hashes(directory):
    return {name: hashlib.sha256((directory / name).read_bytes()).hexdigest() for name in OBJECTS}


def check(lab, output):
    output.mkdir()
    records = []

    def command(name, *arguments, success=True):
        result = subprocess.run([str(lab), *map(str, arguments)], capture_output=True, timeout=45)
        (output / f"{name}.stdout").write_bytes(result.stdout)
        (output / f"{name}.stderr").write_bytes(result.stderr)
        records.append({"name": name, "exit_code": result.returncode, "expected_success": success})
        assert (result.returncode == 0) == success, name
        return result

    audio = Path("testdata/synthetic/media-import/stereo-canonical.ogg").resolve()
    authoring = output / "authoring.json"
    authoring.write_text(json.dumps({"schema_version": 1, "song_id": "quic-loopback-fixture",
        "ruleset_id": "duo-watermark-v1", "source_note": "Original bounded stereo test fixture",
        "anchors": [{"id": 1, "frame": 1200}], "sections": []}))
    package = output / "source"
    built = command("build-fixture", "build-authored-package", audio, 4800, authoring, package)
    identity = "package-blake3:" + re.search(rb"Package BLAKE3: ([0-9a-f]{64})", built.stdout)[1].decode()
    original = hashes(package)
    template = output / "replay.json"
    facts = []
    for player in (1, 2):
        for seq in range(70):
            facts.append({"type": "hit", "epoch": 1, "player": player, "seq": seq,
                          "song_time_frames": 100 + seq * 50 + (player - 1) * 5})
        facts.append({"type": "watermark", "epoch": 1, "player": player, "through_frames": 25681})
    document = {"format": "CoCoBeat Replay", "version": 1, "content_id": identity,
                "rules_id": "duo-watermark-v1", "build_id": "quic-session-check", "epoch": 1, "facts": facts}
    template.write_text(json.dumps(document))

    def pair(name, receive, guest_template=template, success=True, source=package):
        invite = output / f"{name}-invite.json"
        host_output = output / f"{name}-host"
        guest_output = output / f"{name}-guest"
        destination = output / f"{name}-received" if receive else source
        with (output / f"{name}-host.stdout").open("wb") as stdout, (output / f"{name}-host.stderr").open("wb") as stderr:
            host = subprocess.Popen([str(lab), "net-host", str(source), str(template), "127.0.0.1:0", str(invite), str(host_output)], stdout=stdout, stderr=stderr)
            try:
                deadline = time.monotonic() + 10
                while not invite.exists():
                    assert host.poll() is None, f"{name}: host exited before invitation"
                    assert time.monotonic() < deadline, f"{name}: invitation timeout"
                    time.sleep(0.01)
                command(f"{name}-guest", "net-receive" if receive else "net-join", destination, guest_template, invite, guest_output, success=success)
                code = host.wait(timeout=40)
                records.append({"name": f"{name}-host", "exit_code": code, "expected_success": success})
                assert (code == 0) == success, f"{name}: host status"
            finally:
                if host.poll() is None:
                    host.terminate()
                    host.wait(timeout=5)
        if receive:
            assert hashes(destination) == hashes(source), f"{name}: original bytes changed"
            command(f"{name}-verify", "verify-package", destination)
        host_status = json.loads((host_output / "status.json").read_text())
        guest_status = json.loads((guest_output / "status.json").read_text())
        if success:
            assert host_status["status"] == guest_status["status"] == "COMPLETE"
            assert host_status["facts"] == guest_status["facts"] == [71, 71]
            assert host_status["event_count"] == guest_status["event_count"]
            if host_status["protocol_version"] >= 3:
                host_timing = host_status["network_timing"]
                guest_timing = guest_status["network_timing"]
                assert host_timing["host_start_ns"] == guest_timing["host_start_ns"]
                assert host_timing["local_start_ns"] == host_timing["host_start_ns"]
                assert host_timing["start_uncertainty_ns"] == 0
                for timing in (host_timing, guest_timing):
                    assert timing["clock"] is not None
                    assert timing["software_start_observed_ns"] >= timing["local_start_ns"]
                    assert timing["software_start_lateness_ns"] == timing["software_start_observed_ns"] - timing["local_start_ns"]
                    assert timing["software_start_lateness_ns"] <= 100_000_000
                assert 1 <= guest_timing["probes_sent"] <= 8
            assert (host_output / "authority.replay.json").read_bytes() == (guest_output / "authority.replay.json").read_bytes()
            assert hashes(source) == original
            command(f"{name}-replay", "inspect-replay", destination, guest_output / "authority.replay.json", output / f"{name}-diagnostics.jsonl")
        else:
            assert host_status["status"] == guest_status["status"] == "FAILED"
            assert host_status["facts"] == guest_status["facts"] == [0, 0]
        return invite, destination

    invite, received = pair("receive", True)
    pair("installed", False)
    wrong = output / "wrong-replay.json"
    wrong.write_text(json.dumps({**document, "content_id": "package-blake3:" + "0" * 64}))
    pair("wrong-template", True, wrong, False)

    command("existing-package", "net-receive", received, template, invite, output / "unused-existing", success=False)
    assert hashes(received) == original
    sentinel = output / "existing-file"
    sentinel.write_bytes(b"keep")
    command("existing-file", "net-receive", sentinel, template, invite, output / "unused-file", success=False)
    assert sentinel.read_bytes() == b"keep"
    existing_output = output / "existing-output"
    existing_output.mkdir()
    (existing_output / "sentinel").write_bytes(b"keep")
    command("existing-output", "net-receive", output / "unused-output-package", template, invite, existing_output, success=False)
    assert (existing_output / "sentinel").read_bytes() == b"keep"
    alias = output / "alias"
    alias.symlink_to(received, target_is_directory=True)
    command("existing-alias", "net-receive", alias, template, invite, output / "unused-alias", success=False)
    assert alias.is_symlink() and hashes(alias) == original
    bad_invite = output / "old-protocol.json"
    invitation = json.loads(invite.read_text())
    invitation["protocol_version"] = 1
    bad_invite.write_text(json.dumps(invitation))
    command("old-protocol", "net-receive", output / "unused-protocol", template, bad_invite, output / "unused-protocol-output", success=False)
    assert not list(output.glob(".cocobeat-package-*")), "staging leak"
    for name in ("unused-existing", "unused-file", "unused-output-package", "unused-alias", "unused-protocol", "unused-protocol-output"):
        assert not (output / name).exists(), f"unexpected {name}"
    assert hashes(package) == original
    summary = {"status": "PASS", "transport": "real QUIC over loopback", "production_binary": str(lab),
               "binary_sha256": hashlib.sha256(lab.read_bytes()).hexdigest(), "commands": records,
               "source_objects_sha256": original, "scope": "headless resources and reliable history; no game window, physical devices or two-machine acceptance"}
    (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(json.dumps({"status": "PASS", "commands": len(records), "evidence": str(output / "summary.json")}))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("lab", type=Path)
    parser.add_argument("new_output", type=Path)
    args = parser.parse_args()
    check(args.lab.resolve(), args.new_output.resolve())
