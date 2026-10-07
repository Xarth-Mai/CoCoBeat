#!/usr/bin/env python3
"""Stdlib Linux QA, never a production analysis backend"""
import bisect
import hashlib
import json
import math
import os
from pathlib import Path
import resource
import signal
import struct
import subprocess
import sys
import time

RATE, HOP, WARMUP, ZERO_SUPPORT = 48000, 128, 131072, 134272


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def write(path, value):
    with path.open("x") as stream:
        json.dump(value, stream, ensure_ascii=False, indent=2, allow_nan=False)
        stream.write("\n")


def verify(prepared):
    for line in (prepared / "source-files.sha256").read_text().splitlines():
        digest, relative = line.split("  ", 1)
        require(sha(prepared / relative) == digest, "prepared source changed: " + relative)


def call(command, prefix, deadline):
    remaining = min(30, deadline - time.monotonic())
    if remaining <= 0:
        raise TimeoutError("total execution budget exhausted")
    environment = dict(os.environ, ASAN_OPTIONS="detect_leaks=1:halt_on_error=1", UBSAN_OPTIONS="halt_on_error=1")
    started, timed_out, process, spawn_error = time.monotonic(), False, None, None
    with prefix.with_suffix(".stdout").open("xb") as stdout, prefix.with_suffix(".stderr").open("xb") as stderr:
        try:
            process = subprocess.Popen(command, stdout=stdout, stderr=stderr, env=environment, start_new_session=True)
            process.wait(timeout=remaining)
        except OSError as error:
            spawn_error = str(error)
        except subprocess.TimeoutExpired:
            timed_out = True
        finally:
            if process is not None and process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait()
    result = {"command": command, "cwd": str(Path.cwd()), "exit_code": process.returncode if process else None,
              "spawn_error": spawn_error,
              "timed_out": timed_out, "elapsed_seconds": time.monotonic() - started,
              "timeout_seconds": remaining, "owned_process_reaped": True,
              "max_child_rss_kib_since_runner_start": resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss,
              "sanitizer_environment": {key: environment[key] for key in ("ASAN_OPTIONS", "UBSAN_OPTIONS")},
              "stdout_sha256": sha(prefix.with_suffix(".stdout")), "stderr_sha256": sha(prefix.with_suffix(".stderr"))}
    write(prefix.with_suffix(".run.json"), result)
    if timed_out:
        raise TimeoutError("process timeout: " + str(prefix))
    return result


def validate(path, frames, channel, block):
    with path.open() as stream:
        header = json.loads(stream.readline())
        require(header == {"type": "header", "algorithm": "btt-48k-tempo-only", "sample_rate": RATE,
                "channels": 2, "channel": channel, "sample_frames": frames, "block_frames": block,
                "fft_frames": 1024, "hop_frames": HOP, "oss_frames": 1024, "filter_order": 15,
                "warmup_frames": WARMUP, "zero_support_frames": ZERO_SUPPORT,
                "latency_adjustments": [0, 0], "callbacks": False, "confidence": None,
                "coordinate": "consumed_pcm_frames_not_event_position",
                "zero_test": "exact_selected_channel_pcm_not_perceptual_silence"}, "header differs")
        stride = max(HOP, block)
        expected = list(range(stride, frames + 1, stride))
        if not expected or expected[-1] != frames:
            expected.append(frames)
        rows = []
        for consumed in expected:
            row = json.loads(stream.readline())
            require(row["type"] == "tempo" and row["consumed_frames"] == consumed
                    and row["warmup_complete"] == (consumed >= WARMUP) and row["confidence"] is None,
                    "incorrect observation coordinate or warmup")
            bpm, period, certainty, zeros = (row[k] for k in ("bpm", "period_frames", "native_certainty", "trailing_zero_frames"))
            require(type(period) is int and type(zeros) is int and 0 <= zeros <= consumed
                    and math.isfinite(bpm) and math.isfinite(certainty) and certainty >= 0, "invalid native value")
            require((bpm == 0 and period == 0) or (period in range(112 * HOP, 451 * HOP, HOP)
                    and math.isclose(bpm, 60 * RATE / period, rel_tol=1e-6)), "tempo/period mismatch")
            require(consumed >= WARMUP or (bpm == 0 and period == 0), "nonzero tempo during warmup")
            stale = "not_in_exact_zero_run" if not zeros else "no_retained_tempo" if not period else (
                "stale_after_full_zero_support" if zeros >= ZERO_SUPPORT else "insufficient_zero_history")
            require(row["stale_status"] == stale, "incorrect exact-zero status")
            rows.append(row)
        require(json.loads(stream.readline()) == {"type": "complete", "sample_frames": frames, "records": len(rows)}
                and stream.read() == "", "missing completion or extra records")
    return rows


def distribution(values):
    ordered = sorted(values)
    return {name: ordered[math.ceil(q * len(ordered)) - 1] if ordered else None
            for name, q in (("p05", .05), ("p50", .5), ("p95", .95), ("max", 1))}


def statistics(case, channel, rows):
    beats = [round(seconds * RATE) for seconds in case["beat_seconds"]]
    require(all(0 <= b < case["frames"] for b in beats)
            and all(a < b for a, b in zip(beats, beats[1:])), "invalid original beat coordinates")
    silent = channel in case["silent_channels"]
    reference_reads, errors, ratios = 0, [], []
    for row in rows:
        index = bisect.bisect_right(beats, row["consumed_frames"]) - 1
        if not silent and row["warmup_complete"] and 0 <= index < len(beats) - 1:
            reference_reads += 1
            if row["bpm"]:
                bpm = 60 * RATE / (beats[index + 1] - beats[index])
                ratios.append(row["bpm"] / bpm)
                errors.append(abs(row["bpm"] - bpm) / bpm * 100)
    return {"quality_status": "UNSCORED_NO_ADMISSION_THRESHOLD", "reference_beat_unit": case["reference_beat_unit"],
            "reference_kind": "silent_channel" if silent else "variable_interval_mean" if case["name"] == "accelerando_90_150" else "stable_recipe_interval_mean",
            "reference_readings_after_warmup": reference_reads, "nonzero_reference_readings": len(errors),
            "nonzero_coverage": len(errors) / reference_reads if reference_reads else None,
            "relative_error_percent": distribution(errors), "raw_bpm_over_reference": distribution(ratios),
            "first_nonzero_consumed_frame": next((r["consumed_frames"] for r in rows if r["bpm"]), None),
            "silent_channel_nonzero_readings": sum(r["bpm"] != 0 for r in rows) if silent else None,
            "exact_zero_run_nonzero_readings": sum(r["bpm"] != 0 and r["trailing_zero_frames"] > 0 for r in rows),
            "full_zero_support_stale_readings": sum(r["stale_status"] == "stale_after_full_zero_support" for r in rows),
            "last_reading": rows[-1], "confidence": None, "production_admission": False}


def run_driver(driver, pcm, expected_pcm_sha, frames, channel, block, prefix, deadline):
    before = {str(path): sha(path) for path in (driver, pcm)}
    require(before[str(pcm)] == expected_pcm_sha, "PCM changed before execution")
    write(prefix.with_suffix(".input-before.json"), before)
    prediction = prefix.with_suffix(".jsonl")
    try:
        run = call([str(driver), str(pcm), str(channel), str(frames), str(block), str(prediction)], prefix, deadline)
    finally:
        after = {path: sha(Path(path)) for path in before}
        write(prefix.with_suffix(".input-after.json"), after)
        require(after == before, "driver or PCM changed during execution")
    require(run["exit_code"] == 0, "native process failed: " + str(prefix))
    rows = validate(prediction, frames, channel, block)
    return rows, {"run": run, "input_sha256": before[str(pcm)], "driver_sha256": before[str(driver)],
                  "output_sha256": sha(prediction)}


def controls(driver, output, deadline):
    output.mkdir()
    zero = bytes(1024 * 8)
    cases = [("short", bytes(8), 1, 0, 128, 0), ("silence", zero, 1024, 0, 128, 0),
             ("positive-four", struct.pack("<ff", 4, 4) * 1024, 1024, 0, 128, 0),
             ("negative-four", struct.pack("<ff", -4, -4) * 1024, 1024, 0, 128, 0),
             ("nextup-four", struct.pack("<I", 0x40800001) + zero[4:], 1024, 0, 128, 2),
             ("nan", struct.pack("<f", float("nan")) + zero[4:], 1024, 0, 128, 2),
             ("infinity", struct.pack("<f", float("inf")) + zero[4:], 1024, 0, 128, 2),
             ("unselected-nan", zero[:4] + struct.pack("<f", float("nan")) + zero[8:], 1024, 0, 128, 2),
             ("truncated", zero[:-1], 1024, 0, 128, 2), ("extra", zero + b"\0", 1024, 0, 128, 2),
             ("zero-frames", zero, 0, 0, 128, 2), ("over-bound", zero, 28800001, 0, 128, 2),
             ("wrong-channel", zero, 1024, 2, 128, 2), ("wrong-block", zero, 1024, 0, 127, 2),
             ("existing-output", zero, 1024, 0, 128, 2), ("input-is-output", zero, 1024, 0, 128, 2)]
    results = []
    before_driver = sha(driver)
    write(output / "identity.json", {"driver": str(driver), "sha256": before_driver})
    for name, data, frames, channel, block, expected in cases:
        pcm, prediction = output / (name + ".f32le"), output / (name + ".jsonl")
        with pcm.open("xb") as stream:
            stream.write(data)
        if name == "existing-output":
            prediction.write_bytes(b"do not replace\n")
        if name == "input-is-output":
            prediction = pcm
        run = call([str(driver), str(pcm), str(channel), str(frames), str(block), str(prediction)], output / name, deadline)
        require(run["exit_code"] == expected and pcm.read_bytes() == data, "contract failed: " + name)
        if expected == 0:
            rows = validate(prediction, frames, channel, block)
            require(all(r["bpm"] == 0 and not r["warmup_complete"] for r in rows), "unexpected pre-warmup tempo")
        elif name == "existing-output":
            require(prediction.read_bytes() == b"do not replace\n", "existing output changed")
        elif name != "input-is-output" and prediction.exists():
            require(not any(json.loads(line).get("type") == "complete" for line in prediction.read_text().splitlines()),
                    "rejected input has a completion record")
        results.append({"case": name, "status": "PASS", "pcm_sha256": sha(pcm), "run": run})
    require(sha(driver) == before_driver, "driver changed during controls")
    write(output / "report.json", {"software_status": "PASS", "quality_status": "NOT_RUN", "cases": results})


def pcm_inputs(prepared, recipe, kinds):
    for kind in kinds:
        path = prepared / "frozen" / (kind + "-matrix.json")
        require(sha(path) == recipe["matrices_sha256"][kind], "matrix changed")
        matrix = json.loads(path.read_bytes())
        for case in matrix["cases"]:
            pcm = Path(recipe["pcm_roots"][kind]) / case["pcm"]
            require(not pcm.is_symlink() and pcm.stat().st_size == case["frames"] * 8
                    and sha(pcm) == case["pcm_sha256"], "old PCM differs: " + str(pcm))
            yield kind, case, pcm


def pilot(prepared, output, recipe, deadline):
    drivers = {}
    for mode in ("native", "sanitized"):
        build = output / ("build-" + mode)
        run = call(["bash", str(prepared / "frozen/build.sh"), str(prepared), str(build), mode], output / ("build-" + mode), deadline)
        require(run["exit_code"] == 0, mode + " build failed")
        drivers[mode] = build / "native-tempo-check"
        controls(drivers[mode], output / ("controls-" + mode), deadline)
    # Only the existing fixed_120 whole PCM is used, no excerpt or newly generated music
    _, case, pcm = next(pcm_inputs(prepared, recipe, ["source"]))
    rows, record = run_driver(drivers["native"], pcm, case["pcm_sha256"], case["frames"], 0, 128, output / "pilot-native", deadline)
    records = [{"name": "pilot-native", **record, "statistics": statistics(case, 0, rows)}]
    for mode, block, label in (("native", 1, "block-1"), ("native", 1024, "block-1024"),
                               ("native", 128, "repeat"), ("sanitized", 128, "sanitized")):
        other, record = run_driver(drivers[mode], pcm, case["pcm_sha256"], case["frames"], 0, block, output / label, deadline)
        common = {r["consumed_frames"]: r for r in rows}
        for row in other:
            reference = common[row["consumed_frames"]]
            if mode == "sanitized":
                require(all(row[key] == reference[key] for key in row if key not in ("bpm", "native_certainty"))
                        and all(math.isclose(row[key], reference[key], rel_tol=1e-6, abs_tol=1e-6)
                                for key in ("bpm", "native_certainty")), "sanitized numerical mismatch")
            else:
                require(row == reference, "chunk/repetition mismatch")
        records.append({"name": label, **record})
    return {"software_status": "PASS", "pilot": records,
            "estimated_40_record_native_seconds_from_one_channel": records[0]["run"]["elapsed_seconds"] * 40,
            "estimate_scope": "same 32 s length, excludes build, input hashes and QA statistics; other rhythms may cost differently"}


def matrix(prepared, driver, output, recipe, deadline):
    require((driver.parent / "source-manifest.sha256").read_text().split()[0]
            == sha(prepared / "source-files.sha256"), "driver build does not identify these frozen sources")
    require((driver.parent / "binary.sha256").read_text().split()[0] == sha(driver), "built binary changed")
    inputs = list(pcm_inputs(prepared, recipe, ("source", "canonical")))
    results = []
    for index, (kind, case, pcm) in enumerate(inputs):
        for channel in (0, 1):
            prefix = output / f"{index:02}-channel-{channel}"
            rows, run = run_driver(driver, pcm, case["pcm_sha256"], case["frames"], channel, 128, prefix, deadline)
            result = {"origin": kind, "case": case["name"], "channel": channel,
                      **run, "statistics": statistics(case, channel, rows)}
            write(prefix.with_suffix(".statistics.json"), result)
            results.append(result)
    require(len(results) == recipe["record_count"] == 40, "incomplete matrix")
    require(all(sha(pcm) == case["pcm_sha256"] for _, case, pcm in inputs), "PCM changed during matrix")
    return {"software_status": "PASS", "records": results}


def self_test():
    case = {"name": "meter_6_8", "frames": 200000, "beat_seconds": [0, 2, 3.5],
            "silent_channels": [1], "reference_beat_unit": "dotted_quarter"}
    row = {"consumed_frames": 140000, "warmup_complete": True, "bpm": 80,
           "trailing_zero_frames": 0, "stale_status": "not_in_exact_zero_run"}
    result = statistics(case, 0, [row])
    require(result["relative_error_percent"]["p50"] == 100 and result["raw_bpm_over_reference"]["p50"] == 2
            and result["reference_beat_unit"] == "dotted_quarter", "scalar unit or harmonic preservation failed")
    require(statistics(case, 1, [row])["reference_readings_after_warmup"] == 0, "silent channel was scored")
    require(statistics(case, 0, [{**row, "consumed_frames": 190000}])["nonzero_coverage"] is None,
            "reference extrapolated beyond the last beat")
    require(statistics(case, 0, [{**row, "warmup_complete": False}])["reference_readings_after_warmup"] == 0,
            "warmup was scored")
    require(distribution([])["p50"] is None, "empty distribution fabricated data")
    print("scalar reference checks PASS; native/MIR execution NOT RUN")


def main():
    args = sys.argv[1:]
    if args == ["self-test"]:
        self_test()
        return 0
    require((len(args) == 3 and args[0] == "pilot") or (len(args) == 4 and args[0] == "matrix"),
            "usage: check.py self-test | pilot PREPARED NEW_OUTPUT | matrix PREPARED DRIVER NEW_OUTPUT")
    prepared, output = Path(args[1]).resolve(), Path(args[-1]).resolve()
    output.mkdir()
    started = time.monotonic()
    deadline = started + (60 if args[0] == "pilot" else 300)
    identity = {"prepared": str(prepared), "source_manifest_sha256": sha(prepared / "source-files.sha256"),
                "qa_sha256": sha(Path(__file__)), "mode": args[0]}
    write(output / "identity.json", identity)
    status, code = {}, 0
    try:
        verify(prepared)
        require(sha(Path(__file__)) == sha(prepared / "frozen/check.py"), "runner differs from frozen source")
        recipe = json.loads((prepared / "recipe.json").read_bytes())
        status = pilot(prepared, output, recipe, deadline) if args[0] == "pilot" else matrix(
            prepared, Path(args[2]).resolve(), output, recipe, deadline)
        verify(prepared)
        require(sha(prepared / "source-files.sha256") == identity["source_manifest_sha256"], "source manifest changed")
        require(time.monotonic() <= deadline, "total execution budget exhausted")
    except (OSError, ValueError, TimeoutError, KeyError, IndexError) as error:
        status, code = {"software_status": "NOT_COMPLETED", "error": str(error)}, 2
    try:
        verify(prepared)
        require(sha(prepared / "source-files.sha256") == identity["source_manifest_sha256"], "source manifest changed")
        status["frozen_sources_unchanged"] = True
    except (OSError, ValueError) as error:
        status.update(frozen_sources_unchanged=False, source_integrity_error=str(error), software_status="NOT_COMPLETED")
        code = 2
    write(output / "report.json", {**status, "quality_status": "UNSCORED_NO_ADMISSION_THRESHOLD",
          "production_admission": False, "elapsed_seconds": time.monotonic() - started, "owned_children_remaining": [],
          "performance_scope": "diagnostic_not_isolated_benchmark"})
    return code


if __name__ == "__main__":
    raise SystemExit(main())
