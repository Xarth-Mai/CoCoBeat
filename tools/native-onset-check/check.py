#!/usr/bin/env python3
"""Stdlib QA runner, never a product analysis backend"""
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import sys
import time


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    with path.open("x") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write("\n")


def call(command, prefix):
    started = time.monotonic()
    with prefix.with_suffix(".stdout").open("xb") as stdout, prefix.with_suffix(".stderr").open("xb") as stderr:
        try:
            result = subprocess.run(command, stdout=stdout, stderr=stderr, timeout=5, check=False)
            code = result.returncode
        except subprocess.TimeoutExpired:
            code = "TIMEOUT"
    record = {"command": command, "exit_code": code, "elapsed_seconds": time.monotonic() - started,
              "stdout_sha256": sha(prefix.with_suffix(".stdout")), "stderr_sha256": sha(prefix.with_suffix(".stderr"))}
    write(prefix.with_suffix(".run.json"), record)
    return record


def validate(path, frames, channels, channel):
    result = json.loads(path.read_text())
    assert result["sample_rate"] == 48000 and result["sample_frames"] == frames
    assert result["channels"] == channels and result["channel"] == channel
    assert result["window_frames"] == 1024 and result["hop_frames"] == 128
    assert result["padding"] is False and result["confidence"] is None
    rows = (frames - 1024) // 128 + 1
    novelty = result["novelty"]
    assert len(novelty) == rows and all(math.isfinite(v) and 0 <= v <= 1 for v in novelty)
    predicted = result["predicted_frames"]
    assert predicted == sorted(set(predicted))
    assert all(type(v) is int and 0 <= v < frames and v % 128 == 0 and v // 128 < rows for v in predicted)
    return result


def controls(driver, output):
    base = struct.pack("<f", 0.0) * 1024
    cases = [
        ("silence", base, 1, 0, 1024, 0),
        ("positive-four", struct.pack("<f", 4.0) * 1024, 1, 0, 1024, 0),
        ("negative-four", struct.pack("<f", -4.0) * 1024, 1, 0, 1024, 0),
        ("nextup-four", struct.pack("<I", 0x40800001) + base[4:], 1, 0, 1024, 2),
        ("nan", struct.pack("<f", float("nan")) + base[4:], 1, 0, 1024, 2),
        ("infinity", struct.pack("<f", float("inf")) + base[4:], 1, 0, 1024, 2),
        ("unselected-nan", (struct.pack("<ff", 0.0, float("nan"))) * 1024, 2, 0, 1024, 2),
        ("truncated", base[:-1], 1, 0, 1024, 2),
        ("extra", base + b"\0", 1, 0, 1024, 2),
        ("short", struct.pack("<f", 0.0), 1, 0, 1, 2),
        ("over-budget", base, 1, 0, 3072001, 2),
        ("existing-output", base, 1, 0, 1024, 2),
    ]
    results = []
    for name, data, channels, channel, frames, expected in cases:
        pcm, prediction = output / f"{name}.f32le", output / f"{name}.json"
        with pcm.open("xb") as stream:
            stream.write(data)
        if name == "existing-output":
            with prediction.open("xb") as stream:
                stream.write(b"do not replace\n")
        record = call([str(driver), str(pcm), str(channels), str(channel), str(frames), str(prediction)], output / name)
        assert record["exit_code"] == expected, record
        assert pcm.read_bytes() == data
        if expected == 0:
            parsed = validate(prediction, frames, channels, channel)
            assert parsed["predicted_frames"] == [] and parsed["novelty"] == [0]
        elif name == "existing-output":
            assert prediction.read_bytes() == b"do not replace\n"
        else:
            assert not prediction.exists()
        results.append({"case": name, "status": "PASS", "pcm_sha256": sha(pcm), "run": record})
    return {"software_status": "PASS", "quality_status": "NOT_RUN", "cases": results}


def matrix(driver, matcher, prepared, fixtures, output):
    recipe = json.loads((prepared / "recipe.json").read_text())
    declaration = prepared / "frozen/testdata/synthetic/mir-flux-gate-probe/declared-band-background-20261003.json"
    assert sha(declaration) == recipe["declaration_sha256"]
    # All historical PCM hashes must match before any candidate inference is started
    for item in recipe["inputs"]:
        pcm = fixtures / item["regenerated_filename"]
        assert pcm.stat().st_size == item["pcm_bytes"] and sha(pcm) == item["pcm_sha256"], str(pcm)
    results = []
    for index, item in enumerate(recipe["inputs"]):
        pcm = fixtures / item["regenerated_filename"]
        prediction, score = output / f"{index:02}.json", output / f"{index:02}-score.json"
        record = call([str(driver), str(pcm), str(item["channels"]), str(item["channel"]), str(item["sample_frames"]), str(prediction)], output / f"{index:02}")
        if record["exit_code"] == 2 and item["sample_frames"] < 1024:
            assert not prediction.exists()
            assert "UNSUPPORTED: fewer than 1024" in (output / f"{index:02}.stderr").read_text()
            metrics, status = None, "FAIL_UNSUPPORTED"
        else:
            assert record["exit_code"] == 0, record
            validate(prediction, item["sample_frames"], item["channels"], item["channel"])
            scored = call([str(matcher), "score", str(declaration), str(index), str(prediction), str(score)], output / f"{index:02}-score")
            assert scored["exit_code"] == 0, scored
            metrics = json.loads(score.read_text())
            status = metrics["status"] if metrics is not None else "UNSCORED"
        assert sha(pcm) == item["pcm_sha256"]
        results.append({"case": item["case"], "status": status, "metrics": metrics, "run": record,
                        "pcm_sha256": item["pcm_sha256"], "truth_frames": item["truth_frames"]})
    return {"software_status": "PASS", "quality_status": "FAIL" if any(r["status"].startswith("FAIL") for r in results) else "PASS",
            "cases": results, "canonical_same_inputs": "NOT_RUN", "production_admission": False}


def main():
    args = sys.argv[1:]
    if not (len(args) == 3 and args[0] == "controls" or len(args) == 6 and args[0] == "matrix"):
        raise SystemExit("usage: check.py controls DRIVER NEW_OUTPUT | matrix DRIVER MATCHER PREPARED FIXTURES NEW_OUTPUT")
    driver, output = Path(args[1]).resolve(), Path(args[-1]).resolve()
    output.mkdir()
    before = sha(driver)
    write(output / "identity.json", {"driver": str(driver), "driver_sha256": before, "qa_sha256": sha(Path(__file__))})
    if args[0] == "controls":
        report = controls(driver, output)
    else:
        report = matrix(driver, *(Path(p).resolve() for p in args[2:5]), output)
    assert sha(driver) == before
    write(output / "report.json", {**report, "binary_unchanged": True})
    return 1 if report["quality_status"] == "FAIL" else 0


if __name__ == "__main__":
    raise SystemExit(main())
