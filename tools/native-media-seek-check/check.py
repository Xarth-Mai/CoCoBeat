#!/usr/bin/env python3
"""Compare canonical seek windows with two complete decoders, without playback"""
import argparse
from array import array
import csv
import hashlib
import io
import json
import math
from pathlib import Path
import random
import subprocess
import sys
import traceback

REPO = Path(__file__).resolve().parents[2]
CASES = ("short-1", "short-1024", "short-1025", "original-48000",
         "original-64s", "original-600s", "head-tail-impulses", "silence-48000")
WINDOW = 4096


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def save(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, ensure_ascii=False, indent=2, allow_nan=False)
        output.write("\n")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def floats(raw):
    require(len(raw) % 8 == 0, "Partial stereo F32LE frame")
    values = array("f", raw)
    require(values.itemsize == 4, "Requires 32-bit float arrays")
    if sys.byteorder != "little":
        values.byteswap()
    return values


def compare(actual, expected):
    finite = all(map(math.isfinite, actual)) and all(map(math.isfinite, expected))
    difference = max((abs(a - b) for a, b in zip(actual, expected)), default=0.0) if finite else None
    return {"status": "PASS" if finite and len(actual) == len(expected)
            and difference <= 1e-6 else "FAIL", "frames": len(actual) // 2,
            "expected_frames": len(expected) // 2, "finite": finite,
            "max_abs_difference": difference}


def run(command, directory, name, timeout=120):
    process = subprocess.run([str(value) for value in command], cwd=REPO,
                             capture_output=True, timeout=timeout, check=False)
    logs = {}
    for stream in ("stdout", "stderr"):
        path = directory / f"{name}.{stream}"
        with path.open("xb") as output:
            output.write(getattr(process, stream))
        logs[str(path.relative_to(directory))] = digest(path)
    save(directory / f"{name}.json", {"command": list(map(str, command)),
         "cwd": str(REPO), "exit_code": process.returncode, "logs": logs})
    return process


def targets(total):
    points = [0, total - 1, 1, total - 1025, 1023, 1024, 1025, total // 2,
              total - 1024, total - 1023, total - 257, total - 256, total - 255,
              255, 256, 257, total - 129, total - 128, total - 127, total,
              0, total // 2, 1, total - 1, 0]
    points = [point for point in points if 0 <= point <= total]
    points += [random.Random(4343 + index).randrange(total + 1) for index in range(64)]
    return points


def check_case(name, case, driver, seek, root):
    output = root / name
    output.mkdir()
    ogg = case / "canonical.ogg"
    input_record = case / "result.json"
    input_record_hash = digest(input_record)
    original = json.loads(input_record.read_text())
    require(original["status"] == "PASS_SOFTWARE_CASE", "Shipping encode case did not pass")
    total = original["encoded"]["frames"]
    expected_hash = original["ogg_sha256"]
    require(digest(ogg) == expected_hash, "Ogg differs from recorded shipping encoder output")
    # Only the observed canonical 2048-frame maximum block is covered by this experiment
    with ogg.open("rb") as source:
        header = source.read(80)
    body = 27 + header[26]
    require(header[body:body + 7] == b"\x01vorbis" and header[body + 28] >> 4 == 11,
            "Canonical 2048-frame maximum block was not observed")
    baselines = {}
    for kind, command in (("strict", [driver, "readback", ogg, total]),
                          ("reference", [driver, "reference", ogg])):
        path = output / f"{kind}.f32le"
        result = run([*command, path], output, kind)
        require(result.returncode == 0 and json.loads(result.stdout)
                == {"frames": total, "channels": 2, "sample_rate": 48000},
                f"Incomplete {kind} baseline")
        require(path.stat().st_size == total * 8, "Complete baseline length mismatch")
        baselines[kind] = path
    baseline_difference = 0.0
    with baselines["strict"].open("rb") as left, baselines["reference"].open("rb") as right:
        while raw := left.read(65536 * 8):
            match = compare(floats(raw), floats(right.read(len(raw))))
            require(match["status"] == "PASS", "Complete baseline readers disagree")
            baseline_difference = max(baseline_difference, match["max_abs_difference"])
    attempts = []
    points = targets(total)
    for preroll in (0, 1024):
        directory = output / f"preroll-{preroll}"
        directory.mkdir()
        command = [seek, ogg, directory, WINDOW, preroll, *points]
        process = run(command, directory, "seek")
        rows = list(csv.DictReader(io.StringIO(process.stdout.decode())))
        require(len(rows) == len(points), "Missing seek attempt records")
        checked = []
        for index, (point, raw) in enumerate(zip(points, rows)):
            row = {key: value if key in ("method", "status") else int(value) if value else None
                   for key, value in raw.items()}
            require(row["index"] == index and row["target"] == point, "Seek row identity mismatch")
            if row["status"] != "PASS":
                checked.append({"status": "FAIL_API", "metadata": row})
                continue
            path = directory / f"seek-{index}-{point}.f32le"
            actual = floats(path.read_bytes())
            length = min(WINDOW, total - point)
            matches = {}
            for kind, baseline in baselines.items():
                with baseline.open("rb") as source:
                    source.seek(point * 8)
                    matches[kind] = compare(actual, floats(source.read(length * 8)))
            from_start = preroll == 1024 and point <= 1024
            method_ok = (row["method"] == "from_start" and row["request"] is None
                         and row["required_ts"] is None and row["actual_ts"] is None) if from_start else (
                         row["method"] == "seek" and row["request"] == row["required_ts"]
                         == max(0, point - preroll) and row["actual_ts"] <= row["request"])
            timing = method_ok and row["output_frames"] == length and row["first_output_frame"] == (
                     point if length else -1) and row["track_start"] + row["delay"] == 0
            checked.append({"status": "PASS" if timing and all(item["status"] == "PASS"
                            for item in matches.values()) else "FAIL_COMPARE", "metadata": row,
                            "timing": timing, "comparisons": matches, "pcm_sha256": digest(path)})
        invalid = []
        for point in (-1, total + 1):
            rejected = run(command[:5] + [point], directory, f"invalid-{point}")
            message = "target must be nonnegative" if point < 0 else "target exceeds declared valid frame count"
            invalid.append({"target": point, "status": "PASS_REJECTED" if rejected.returncode != 0
                            and message.encode() in rejected.stderr else "FAIL"})
        passed = process.returncode == 0 and all(row["status"] == "PASS" for row in checked)
        passed = passed and all(row["status"] == "PASS_REJECTED" for row in invalid)
        attempts.append({"preroll_frames": preroll, "status": "PASS" if passed else "FAIL",
                         "exit_code": process.returncode, "cases": checked, "invalid_targets": invalid})
    require(digest(ogg) == expected_hash, "Canonical Ogg changed during seek controls")
    require(digest(input_record) == input_record_hash, "Shipping input record changed")
    row = {"name": name, "frames": total, "ogg_sha256": expected_hash,
           "shipping_case_record_sha256": input_record_hash,
           "max_block_frames": 2048, "full_readback_max_abs_difference": baseline_difference,
           "baselines_sha256": {kind: digest(path) for kind, path in baselines.items()},
           "targets": points, "attempts": attempts,
           "status": "PASS_WORKAROUND" if attempts[1]["status"] == "PASS" else "FAIL"}
    save(output / "result.json", row)
    # Large complete PCM is removed only after its identity and comparisons are recorded
    for path in baselines.values():
        path.unlink()
    return row


def main():
    if sys.argv[1:] == ["--self-check"]:
        assert compare([0.0, 1.0], [0.0, 1.0])["status"] == "PASS"
        for actual in ([1.0, 0.0], [0.0], [float("nan"), 1.0]):
            assert compare(actual, [0.0, 1.0])["status"] == "FAIL"
        assert all(0 <= point <= 1 for point in targets(1))
        print("PASS: exact-position comparison rejects shifts, truncation and nonfinite values")
        return 0
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("cargo_json", "shipping_build", "portable_cases", "output"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir(parents=True, exist_ok=False)
    artifacts = [json.loads(line) for line in args.cargo_json.read_text().splitlines() if line.startswith("{")]
    symphonia = [row for row in artifacts if row.get("reason") == "compiler-artifact"
                and row["target"]["name"] == "symphonia"]
    require(len(symphonia) == 1, "Require one exact Cargo Symphonia artifact")
    rlib = next(Path(path) for path in symphonia[0]["filenames"] if path.endswith(".rlib"))
    driver = args.shipping_build.resolve() / "bin/native-vorbis-check"
    identity = json.loads((args.shipping_build / "immutable-binaries.json").read_text())
    require(digest(driver) == identity["native-vorbis-check"]["sha256"], "Fixed shipping binary changed")
    source = REPO / "tools/canonical-audio-probe/seek.rs"
    inputs = [source, Path(__file__).resolve(), args.cargo_json.resolve(), rlib, driver]
    before = {str(path): digest(path) for path in inputs}
    dependency_paths = sorted({Path(path) for row in artifacts
                              if row.get("reason") == "compiler-artifact"
                              for path in row["filenames"] if path.endswith(".rlib")})
    dependencies = {str(path): digest(path) for path in dependency_paths}
    seek = root / "seek-bin"
    command = ["rustc", "--edition=2024", "-D", "warnings", "-O", "-C", "panic=abort", source,
               "--extern", f"symphonia={rlib}", "-L", f"dependency={rlib.parent}", "-o", seek]
    build = run(command, root, "build")
    require(build.returncode == 0, "Actual seek helper build failed")
    provenance = {"inputs_before": before, "dependency_rlibs_before": dependencies,
                  "symphonia_artifact": symphonia[0], "seek_binary_sha256": digest(seek),
                  "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
                  "comparison": "Exact absolute frame slice, finite, exact count, <=1e-6; no padding, alignment or gain fit",
                  "coverage": "Observed canonical q10 max block=2048, eight synthetic inputs, same reader reused across targets",
                  "not_run": ["seek in native four-platform CI", "runtime/editor seek wiring", "listening/hardware"]}
    save(root / "provenance.json", provenance)
    rows = []
    for name in CASES:
        try:
            row = check_case(name, args.portable_cases.resolve() / name, driver, seek, root)
        except Exception:
            row = {"name": name, "status": "FAIL", "error": traceback.format_exc()}
            save(root / name / "failure.json", row)
        rows.append(row)
        print(f"{name}: {row['status']}", flush=True)
    after = {str(path): digest(path) for path in inputs}
    require(before == after, "Build/QA input changed during seek controls")
    require(dependencies == {path: digest(Path(path)) for path in dependencies}, "Dependency rlib changed")
    summary = {"status": "PASS_WORKAROUND" if all(row["status"] == "PASS_WORKAROUND" for row in rows) else "FAIL",
               "inputs_after": after, "dependency_rlibs_unchanged": True, "cases": rows}
    save(root / "summary.json", summary)
    return 0 if summary["status"] == "PASS_WORKAROUND" else 1


if __name__ == "__main__":
    sys.exit(main())
