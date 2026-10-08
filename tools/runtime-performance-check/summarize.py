"""Summarize real monotonic native probe rows; no fabricated FPS or GPU counters."""
import argparse
import json
import math
from pathlib import Path


def distribution(intervals):
    if not intervals:
        raise ValueError("no eligible intervals")
    ordered = sorted(intervals)
    streak = longest = 0
    for value in intervals:
        streak = streak + 1 if value > 33_333_334 else 0
        longest = max(streak, longest)
    return {
        "interval_count": len(intervals), "sum_intervals_ns": sum(intervals),
        "aggregate_main_update_hz": len(intervals) * 1e9 / sum(intervals),
        "mean_ms": sum(intervals) / len(intervals) / 1e6,
        **{f"p{percent}_ms": ordered[math.ceil(percent * len(ordered) / 100) - 1] / 1e6 for percent in (50, 90, 95, 99)},
        "max_ms": ordered[-1] / 1e6,
        **{f"over_{name}_ms": sum(value > threshold for value in intervals) for name, threshold in (("16_666667", 16_666_667), ("33_333334", 33_333_334), ("50", 50_000_000), ("100", 100_000_000))},
        "longest_over_33_333334_ms_streak": longest,
    }


def summarize(rows, metadata, result):
    reasons = []
    last = None
    runs = []
    current = []
    for row in rows:
        frame, at = row["frame"], row["monotonic_ns"]
        if type(frame) is not int or type(at) is not int or frame < 0 or at < 0:
            raise ValueError("invalid raw frame/timestamp")
        if last is not None and (frame != last["frame"] + 1 or at <= last["monotonic_ns"]):
            raise ValueError("nonconsecutive frame or nonincreasing timestamp")
        if row["phase"] == "Running":
            current.append(row)
        elif current:
            runs.append(current)
            current = []
        last = row
    if current:
        runs.append(current)
    if metadata["release"] is not True: reasons.append("not a release build")
    if metadata["time_strategy"] != "Automatic": reasons.append("not automatic Time")
    if metadata.get("stage_compiler_version") not in (1, 2, 3): reasons.append("missing supported compiled stage")
    if result["status"] != "COMPLETE": reasons.append("runtime did not complete")
    if result["synthetic_hits"] != result["expected_hits"]: reasons.append("incomplete synthetic hit schedule")
    expected_captures = result.get("requested_captures", [])
    observed_captures = [{"observed_ns": capture["observed_ns"], "player": capture["player"]} for capture in result.get("capture_diagnostics", [])]
    if len(expected_captures) != result["expected_hits"] or expected_captures != observed_captures:
        reasons.append("mixed or incomplete captured input history")
    producer = result.get("capture_producer", {})
    if metadata.get("capture_producer") != "independent_instant_v2" or producer.get("completed") is not True or producer.get("error") is not None:
        reasons.append("independent actual-Instant producer did not complete")
    receipts = producer.get("captures", [])
    if [{"observed_ns": capture["observed_ns"], "player": capture["player"]} for capture in receipts] != expected_captures or [{"song_frame": capture["target_frame"], "player": capture["player"]} for capture in receipts] != metadata.get("schedule"):
        reasons.append("producer capture or target identity mismatch")
    max_age = metadata.get("publication_max_age_ns")
    if type(max_age) is not int or max_age != 250_000_000:
        reasons.append("missing bounded audio publication age")
    for capture in receipts:
        if not all(type(capture.get(key)) is int for key in ("observed_ns", "moment_ns", "deadline_ns", "anchor_ns", "anchor_frame", "target_frame")) or capture["target_frame"] <= 0 or capture["anchor_frame"] < 0:
            reasons.append("invalid producer receipt timing"); break
        if capture["moment_ns"] != capture["observed_ns"]:
            reasons.append("capture original Instant identity mismatch"); break
        recomputed = capture["anchor_ns"] - (-(capture["target_frame"] - capture["anchor_frame"]) * 1_000_000_000 // 48_000)
        if capture["deadline_ns"] != recomputed or not 0 <= capture["deadline_ns"] <= capture["observed_ns"]:
            reasons.append("producer deadline mismatch or backdated capture"); break
        if type(max_age) is not int or not 0 <= capture["observed_ns"] - capture["anchor_ns"] < max_age:
            reasons.append("stale or future audio publication"); break
    adapter = result.get("adapter")
    if not adapter or not adapter.get("adapter"): reasons.append("missing actual adapter")
    elif any(word in adapter["adapter"].lower() for word in ("llvmpipe", "software", 'device_type: cpu')): reasons.append("software adapter")
    coverage = result.get("summary", {})
    if not coverage.get("free_sync") or not coverage.get("anchor_sync") or not all(coverage.get("hits", [])):
        reasons.append("missing actual two-player/free/anchor feedback coverage")
    anchors = coverage.get("anchors", [])
    if len(anchors) != 2 or not all(len(player) == 4 and player[0] and player[1] and not player[2] and player[3] for player in anchors):
        reasons.append("missing actual Precise/Good/Miss workload coverage")
    if len(runs) != 1: reasons.append("Running workload is not one contiguous segment")
    ready_at = result.get("ready_at_ns")
    ready = [row for row in rows if ready_at is not None and ready_at + 5e9 <= row["monotonic_ns"] <= ready_at + 15e9 and row["phase"] == "Ready" and row["brand_complete"]]
    phases = {}
    for name, selected, seconds in (("ready", ready, 10), ("running", runs[0] if len(runs) == 1 else [], 50)):
        if name == "running" and selected:
            first = selected[0]["monotonic_ns"]
            selected = [row for row in selected if first + 10e9 <= row["monotonic_ns"] <= first + 60e9]
        if len(selected) < 2 or selected[-1]["monotonic_ns"] - selected[0]["monotonic_ns"] < (seconds - 0.25) * 1e9:
            reasons.append(f"incomplete {name} observation window")
        if len(selected) >= 2:
            intervals = [right["monotonic_ns"] - left["monotonic_ns"] for left, right in zip(selected, selected[1:])]
            phases[name] = {**distribution(intervals), "first_frame": selected[0]["frame"], "last_frame": selected[-1]["frame"], "span_ns": selected[-1]["monotonic_ns"] - selected[0]["monotonic_ns"], "spikes": [{"frame": right["frame"], "song_frame": right["song_frame"], "interval_ms": value / 1e6} for right, value in zip(selected[1:], intervals) if value > 33_333_334]}
            if name == "running":
                if not {"Curve", "Bridge"}.issubset({row.get("stage_kind") for row in selected}): reasons.append("steady Running did not cover actual Curve and Bridge")
                if not any(row.get("section_cue_id") is not None for row in selected): reasons.append("steady Running did not cover an authored SectionCue")
            for row in selected:
                expected = metadata["requested_physical_size"]
                if [row["physical_width"], row["physical_height"]] != expected or [row["render_width"], row["render_height"]] != expected or row["scale_factor"] != 1:
                    reasons.append(f"actual {name} physical/render size or scale mismatch"); break
            if any(not row["focused"] or row["display_pending"] for row in selected): reasons.append(f"unstable {name} focus/display")
            if any(row["quality"] != metadata["quality"] for row in selected): reasons.append(f"{name} quality mismatch")
            requested = metadata["pacing"]
            if any(row["vsync"] != requested["vsync"] for row in selected): reasons.append(f"{name} vsync mismatch")
            expected_present = "AutoVsync" if requested["vsync"] else "AutoNoVsync"
            if any(row["window_requested_present_mode"] != expected_present for row in selected): reasons.append(f"{name} requested present mode mismatch")
            frame_limit = requested["frame_limit"]
            expected_limit = "Unlimited" if frame_limit == "unlimited" else f"Limited({frame_limit['limited']})"
            if any(row["frame_limit"] != expected_limit for row in selected): reasons.append(f"{name} frame limit mismatch")
    return {"status": "VALID_LOCAL_OBSERVATION" if not reasons else "INVALID_LOCAL_OBSERVATION", "invalid_reasons": list(dict.fromkeys(reasons)), "scope": "native main-update cadence, actual Kira/core workload, Linux owned-process RSS; not monitor presentation or GPU time", "budget": "DESCRIPTIVE; no hardware budget claimed", "raw_rows": len(rows), "phases": phases, "probe_install_seconds_to_stable_ready": ready_at / 1e9 if ready_at is not None else None, "not_measured": metadata["not_measured"]}


def self_check():
    result = distribution([10_000_000, 20_000_000, 160_000_000])
    assert result["p50_ms"] == 20 and result["p99_ms"] == 160
    assert result["sum_intervals_ns"] == 190_000_000
    assert result["over_50_ms"] == result["over_100_ms"] == 1
    rows = [{"frame": 0, "monotonic_ns": 0, "phase": "Ready"}, {"frame": 1, "monotonic_ns": 1, "phase": "Running"}]
    meta = {"release": False, "time_strategy": "ManualDuration", "pacing": {}, "not_measured": [], "requested_physical_size": [1280, 800]}
    outcome = summarize(rows, meta, {"status": "FAILED", "synthetic_hits": 0, "expected_hits": 1})
    assert outcome["status"] == "INVALID_LOCAL_OBSERVATION" and len(outcome["invalid_reasons"]) >= 7
    for broken in ([rows[0], rows[0]], [rows[0], {**rows[1], "frame": 2}], [rows[0], {**rows[1], "monotonic_ns": 0}]):
        try: summarize(broken, meta, {"status": "FAILED", "synthetic_hits": 0, "expected_hits": 1})
        except ValueError: continue
        raise AssertionError("malformed raw rows accepted")
    common = {"brand_complete": True, "focused": True, "display_pending": False, "physical_width": 1280, "physical_height": 800, "render_width": 1280, "render_height": 800, "scale_factor": 1, "quality": {"preset": "medium"}, "vsync": False, "frame_limit": "Unlimited", "window_requested_present_mode": "AutoNoVsync", "section_cue_id": 1}
    native_like = [{**common, "frame": frame, "monotonic_ns": frame * 10_000_000, "phase": "Ready" if frame <= 1500 else "Running" if frame <= 7900 else "Finished", "song_frame": max(0, frame - 1501) * 480, "stage_kind": "Curve" if frame < 4000 else "Bridge"} for frame in range(8000)]
    valid_meta = {"release": True, "time_strategy": "Automatic", "stage_compiler_version": 2, "requested_physical_size": [1280, 800], "quality": {"preset": "medium"}, "pacing": {"frame_limit": "unlimited", "vsync": False}, "not_measured": ["GPU elapsed"], "capture_producer": "independent_instant_v2", "publication_max_age_ns": 250_000_000, "schedule": [{"song_frame": 1, "player": "P1"}, {"song_frame": 2, "player": "P2"}]}
    captures = [{"observed_ns": 1, "player": "P1"}, {"observed_ns": 2, "player": "P2"}]
    valid_result = {"status": "COMPLETE", "synthetic_hits": 2, "expected_hits": 2, "requested_captures": captures, "capture_diagnostics": captures, "ready_at_ns": 0, "adapter": {"adapter": "DiscreteGpu Vulkan"}, "summary": {"free_sync": 1, "anchor_sync": 1, "hits": [1, 1], "anchors": [[1, 1, 0, 1], [1, 1, 0, 1]]}}
    receipts = [{**capture, "target_frame": index + 1, "moment_ns": capture["observed_ns"], "deadline_ns": 0, "anchor_ns": 0, "anchor_frame": index + 1} for index, capture in enumerate(captures)]
    valid_result["capture_producer"] = {"completed": True, "error": None, "captures": receipts}
    assert summarize(native_like, valid_meta, valid_result)["status"] == "VALID_LOCAL_OBSERVATION"
    assert summarize(native_like, {**valid_meta, "stage_compiler_version": 3}, valid_result)["status"] == "VALID_LOCAL_OBSERVATION"
    assert "missing supported compiled stage" in summarize(native_like, {**valid_meta, "stage_compiler_version": 4}, valid_result)["invalid_reasons"]
    for replacement in ({"quality": {"preset": "low"}}, {"physical_width": 640}, {"window_requested_present_mode": "AutoVsync"}, {"stage_kind": "Straight"}, {"section_cue_id": None}):
        changed = [{**row, **replacement} if row["phase"] == "Running" else row for row in native_like]
        assert summarize(changed, valid_meta, valid_result)["status"] == "INVALID_LOCAL_OBSERVATION"
    mixed = {**valid_result, "capture_diagnostics": captures + [{"observed_ns": 3, "player": "P1"}]}
    assert "mixed or incomplete captured input history" in summarize(native_like, valid_meta, mixed)["invalid_reasons"]
    missing = {**valid_result, "capture_diagnostics": captures[:1]}
    assert "mixed or incomplete captured input history" in summarize(native_like, valid_meta, missing)["invalid_reasons"]
    for replacement in ({"observed_ns": -1}, {"moment_ns": 5}, {"deadline_ns": 2}, {"anchor_ns": 5}, {"target_frame": 0}, {"anchor_frame": -1}):
        broken = {**valid_result, "capture_producer": {"completed": True, "error": None, "captures": [{**receipts[0], **replacement}, receipts[1]]}}
        assert summarize(native_like, valid_meta, broken)["status"] == "INVALID_LOCAL_OBSERVATION"
    assert summarize(native_like, {**valid_meta, "publication_max_age_ns": 500_000_000}, valid_result)["status"] == "INVALID_LOCAL_OBSERVATION"
    failed_worker = {**valid_result, "capture_producer": {"completed": False, "error": "Stale audio capture publication", "captures": receipts}}
    assert summarize(native_like, valid_meta, failed_worker)["status"] == "INVALID_LOCAL_OBSERVATION"
    print("PASS: interval statistics, constructed valid workload and identity/scene/input rejection controls; native performance NOT RUN")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", nargs="?", type=Path)
    args = parser.parse_args()
    if args.directory is None: self_check()
    else:
        directory = args.directory
        value = summarize([json.loads(line) for line in (directory / "frames.jsonl").read_text().splitlines()], json.loads((directory / "metadata.json").read_text()), json.loads((directory / "result.json").read_text()))
        with (directory / "summary.json").open("x") as stream:
            json.dump(value, stream, indent=2, allow_nan=False); stream.write("\n")
        print(json.dumps({"status": value["status"], "invalid_reasons": value["invalid_reasons"]}))
