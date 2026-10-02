#!/usr/bin/env python3
"""Bounded ten-minute and signal-boundary checks for the experimental adapter"""
import argparse
import array
import filecmp
import hashlib
import json
import math
import os
from pathlib import Path
import resource
import struct
import subprocess
import sys
import tempfile
import time

from run import ogg_pages

BLOCK_BYTES = 512 * 1024
ADDRESS_SPACE_BYTES = 2 * 1024**3
MATRIX = {"silence": (48000, "silence"), "near-full": (48000, "near-full"),
          "edge-silence": (144000, "edge-silence"), "long": (48000 * 600, "tail")}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def bounded(command, case, label, seconds=600):
    argv = [str(arg) for arg in command]
    started = time.monotonic()
    with (case / f"{label}.stdout").open("w") as out, (case / f"{label}.stderr").open("w") as err:
        process = subprocess.Popen(["timeout", "--kill-after=5s", f"{seconds}s", *argv], stdout=out, stderr=err,
                                   preexec_fn=lambda: resource.setrlimit(resource.RLIMIT_AS, (ADDRESS_SPACE_BYTES, ADDRESS_SPACE_BYTES)))
        _, status, usage = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
    result = {"argv": argv, "exit_code": process.returncode, "wall_seconds": time.monotonic() - started,
              "user_seconds": usage.ru_utime, "system_seconds": usage.ru_stime, "max_rss_kib": usage.ru_maxrss,
              "wall_timeout_seconds": seconds, "kill_grace_seconds": 5, "address_space_limit_bytes": ADDRESS_SPACE_BYTES}
    write_json(case / f"{label}.resources.json", result)
    return result


def values(raw):
    data = array.array("f", raw[:len(raw) // 8 * 8])
    if sys.byteorder != "little":
        data.byteswap()
    return data


def accumulator():
    return {"samples": 0, "nonfinite": 0, "peak": 0.0, "energy": 0.0, "over_unity": 0, "nonzero": 0}


def update(total, samples):
    for value in samples:
        total["samples"] += 1
        if not math.isfinite(value):
            total["nonfinite"] += 1
            continue
        magnitude = abs(value)
        total["peak"] = max(total["peak"], magnitude)
        total["energy"] += value * value
        total["over_unity"] += magnitude > 1.0
        total["nonzero"] += value != 0.0


def metrics(path, regions):
    size = path.stat().st_size
    totals = {"whole": [accumulator(), accumulator()], **{name: [accumulator(), accumulator()] for name in regions}}
    offset = 0
    with path.open("rb") as source:
        while raw := source.read(BLOCK_BYTES):
            data = values(raw)
            count = len(data) // 2
            for channel in range(2):
                update(totals["whole"][channel], data[channel::2])
                for name, (start, end) in regions.items():
                    lo, hi = max(start - offset, 0), min(end - offset, count)
                    if lo < hi:
                        update(totals[name][channel], data[lo*2+channel:hi*2:2])
            offset += count
    for channels in totals.values():
        for item in channels:
            item["rms"] = None if item["nonfinite"] else math.sqrt(item["energy"] / max(item["samples"], 1))
    return {"frames": size // 8, "trailing_bytes": size % 8, "channels": totals}


def difference(a, b):
    peak = energy = 0.0
    count = nonfinite = 0
    with a.open("rb") as left, b.open("rb") as right:
        while raw := left.read(BLOCK_BYTES):
            for x, y in zip(values(raw), values(right.read(BLOCK_BYTES))):
                count += 1
                if not (math.isfinite(x) and math.isfinite(y)):
                    nonfinite += 1
                    continue
                error = x - y
                peak = max(peak, abs(error))
                energy += error * error
    return {"same_byte_count": a.stat().st_size == b.stat().st_size, "samples_compared": count,
            "nonfinite_pairs": nonfinite, "peak_difference": peak,
            "rms_difference": None if nonfinite else math.sqrt(energy / max(count, 1))}


def compare(source, decoded):
    size = source.stat().st_size
    if not size or size % 8 or decoded.stat().st_size != size:
        raise ValueError("comparison requires equal nonempty complete stereo F32LE frames")
    frames = size // 8
    regions = {"interior": (2048, max(2048, frames-2048))}
    original, output = metrics(source, regions), metrics(decoded, regions)
    if any(c["nonfinite"] for pcm in [original, output] for c in pcm["channels"]["whole"]):
        raise ValueError("comparison requires finite source and decoded samples")
    windows = {"whole": (0, frames), **regions}
    errors = {name: [0.0, 0.0] for name in windows}
    offset = 0
    with source.open("rb") as left, decoded.open("rb") as right:
        while raw := left.read(BLOCK_BYTES):
            a, b = values(raw), values(right.read(BLOCK_BYTES))
            if len(a) != len(b):
                raise ValueError("comparison input length changed while reading")
            count = len(a) // 2
            for name, (start, end) in windows.items():
                lo, hi = max(0, start-offset), min(count, end-offset)
                for channel in range(2):
                    errors[name][channel] += sum((a[i]-b[i])**2 for i in range(lo*2+channel, hi*2, 2))
            offset += count
    result = {"frames": frames, "source": original, "decoded": output, "aligned_error": {}}
    for name, channels in errors.items():
        result["aligned_error"][name] = []
        for channel, error in enumerate(channels):
            truth = original["channels"][name][channel]
            result["aligned_error"][name].append({"samples": truth["samples"], "error_energy": error,
                "rmse": math.sqrt(error/truth["samples"]) if truth["samples"] else None,
                "snr_db": 10*math.log10(truth["energy"]/error) if truth["energy"] > 0 and error > 0 else None})
    return result


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def case(root, binaries, name):
    frames, signal = MATRIX[name]
    dest = root / name
    dest.mkdir(parents=True, exist_ok=True)
    result = {"name": name, "frames": frames, "signal": signal, "q": 5}
    result["encode"] = bounded([binaries / "adapter", dest, frames, 5, signal], dest, "encode")
    if result["encode"]["exit_code"] != 0:
        result["status"] = "FAIL_ENCODE_OR_RESOURCE"
        write_json(dest / "result.json", result)
        return result
    result["symphonia"] = bounded([binaries / "readback", dest / "encoded.ogg", dest / "symphonia.f32le"], dest, "symphonia", 180)
    result["ffmpeg"] = bounded(["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "warning", "-xerror", "-err_detect", "explode", "-c:a", "vorbis", "-i", dest / "encoded.ogg", "-map", "0:a:0", "-vn", "-c:a", "pcm_f32le", "-f", "f32le", "-y", dest / "ffmpeg.f32le"], dest, "ffmpeg", 180)
    result["ffprobe"] = bounded(["ffprobe", "-v", "error", "-show_streams", "-of", "json", dest / "encoded.ogg"], dest, "ffprobe", 30)
    regions = {"first_2048": (0, 2048), "last_2048": (max(0, frames-2048), frames)}
    if signal == "edge-silence":
        regions.update({"leading_silence": (0, 4800), "trailing_silence": (frames-4800, frames),
                        "leading_far_silence": (0, 4800-2048), "trailing_far_silence": (frames-4800+2048, frames),
                        "active": (4800, frames-4800)})
    result["regions"] = regions
    result["pcm"] = {name: metrics(dest / f"{name}.f32le", regions) for name in ["input", "symphonia", "ffmpeg"] if (dest / f"{name}.f32le").exists()}
    pages = ogg_pages(dest / "encoded.ogg")
    result["ogg"] = {"eos_granules": [p["granule"] for p in pages if p["flags"] & 4], "bytes": (dest / "encoded.ogg").stat().st_size, "sha256": digest(dest / "encoded.ogg")}
    checks = {"eos_frames": result["ogg"]["eos_granules"] == [frames], "input_frames": result["pcm"]["input"]["frames"] == frames,
              "input_legal": all(ch["nonfinite"] == 0 and ch["peak"] <= 1 for ch in result["pcm"]["input"]["channels"]["whole"])}
    for decoder in ["symphonia", "ffmpeg"]:
        decoded = result["pcm"].get(decoder, {})
        metadata = {}
        if result[decoder]["exit_code"] == 0:
            if decoder == "symphonia":
                metadata = json.loads((dest / "symphonia.stdout").read_text())
            elif result["ffprobe"]["exit_code"] == 0:
                streams = json.loads((dest / "ffprobe.stdout").read_text()).get("streams", [])
                if len(streams) == 1:
                    metadata = {"sample_rate": int(streams[0]["sample_rate"]), "channels": streams[0]["channels"]}
        result[decoder]["format"] = metadata
        checks[decoder + "_exit"] = result[decoder]["exit_code"] == 0
        checks[decoder + "_format"] = metadata.get("sample_rate") == 48000 and metadata.get("channels") == 2
        checks[decoder + "_frames"] = decoded.get("frames") == frames and decoded.get("trailing_bytes") == 0
        checks[decoder + "_finite"] = bool(decoded) and all(ch["nonfinite"] == 0 for ch in decoded["channels"]["whole"])
    if all((dest / f"{name}.f32le").exists() for name in ["symphonia", "ffmpeg"]):
        result["decoder_agreement"] = difference(dest / "symphonia.f32le", dest / "ffmpeg.f32le")
    result["checks"] = checks
    result["status"] = "PASS_STRUCTURAL" if all(checks.values()) else "FAIL_STRUCTURAL"
    write_json(dest / "result.json", result)
    return result


def regression(root, binaries, reference):
    results = []
    for frames, pulse in [(1, False), (1024, False), (1025, False), (48000, False), (48128, False), (48000, True), (3072000, False)]:
        name = f"{frames}-q5" + ("-pulse" if pulse else "")
        dest = root / "regression" / name
        dest.mkdir(parents=True, exist_ok=True)
        command = [binaries / "adapter", dest, frames, 5] + (["pulse"] if pulse else [])
        operations = [bounded(command, dest, "encode")]
        operations.append(bounded([binaries / "readback", dest / "encoded.ogg", dest / "symphonia.f32le"], dest, "symphonia", 60))
        operations.append(bounded(["ffmpeg", "-nostdin", "-v", "error", "-i", dest / "encoded.ogg", "-map", "0:a:0", "-c:a", "pcm_f32le", "-f", "f32le", "-y", dest / "ffmpeg.f32le"], dest, "ffmpeg", 60))
        files = {}
        for filename in ["input.f32le", "encoded.ogg", "symphonia.f32le", "ffmpeg.f32le"]:
            actual, expected = dest / filename, reference / name / filename
            files[filename] = {"byte_identical": actual.exists() and expected.exists() and filecmp.cmp(actual, expected, shallow=False),
                               "sha256": digest(actual) if actual.exists() else None, "reference_sha256": digest(expected) if expected.exists() else None}
        results.append({"name": name, "operations": operations, "files": files,
                        "status": "PASS" if all(op["exit_code"] == 0 for op in operations) and all(f["byte_identical"] for f in files.values()) else "FAIL"})
    return {"status": "PASS" if all(r["status"] == "PASS" for r in results) else "FAIL", "cases": results}


def self_check():
    global BLOCK_BYTES
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory) / "pcm"
        source.write_bytes(struct.pack("<8f", 1.25, 0, float("nan"), float("inf"), float("-inf"), 0.5, -0.25, 0))
        result = metrics(source, {"middle": (1, 3)})
        assert result["frames"] == 4 and result["trailing_bytes"] == 0
        assert result["channels"]["whole"][0]["peak"] == 1.25
        assert result["channels"]["whole"][0]["over_unity"] == 1
        assert [c["nonfinite"] for c in result["channels"]["whole"]] == [2, 1]
        assert result["channels"]["middle"][0]["samples"] == 2
        previous = BLOCK_BYTES
        try:
            BLOCK_BYTES = 16
            assert metrics(source, {"middle": (1, 3)}) == result
        finally:
            BLOCK_BYTES = previous
        json.dumps(result, allow_nan=False)
        json.dumps(difference(source, source), allow_nan=False)
        source.write_bytes(struct.pack("<2f", 0.5, -0.5) + b"x")
        assert metrics(source, {})["trailing_bytes"] == 1
        decoded = Path(directory) / "decoded"
        source.write_bytes(struct.pack("<2f", 1.0, -0.5) * 4100)
        decoded.write_bytes(struct.pack("<2f", 0.5, -0.25) * 4100)
        previous = BLOCK_BYTES
        try:
            BLOCK_BYTES = 8192
            comparison = compare(source, decoded)
        finally:
            BLOCK_BYTES = previous
        assert [c["rmse"] for c in comparison["aligned_error"]["whole"]] == [0.5, 0.25]
        assert comparison["aligned_error"]["interior"][0]["samples"] == 4
        assert math.isclose(comparison["aligned_error"]["whole"][0]["snr_db"], 10*math.log10(4))
        for invalid in [b"", source.read_bytes()[:-1], source.read_bytes()[:-8], *[struct.pack("<2f", value, 0) * 4100 for value in [float("nan"), float("inf"), float("-inf")]]]:
            decoded.write_bytes(invalid)
            try:
                compare(source, decoded)
            except ValueError:
                pass
            else:
                raise AssertionError("invalid comparison input accepted")
    print("PASS: streaming metrics, known SNR/RMSE, partial or unequal frames and nonfinite comparison rejection")


def main():
    if sys.argv[1:] == ["--self-check"]:
        self_check()
        return
    if sys.argv[1:2] == ["compare"]:
        parser = argparse.ArgumentParser(description="Compare aligned stereo F32LE source and decoded PCM")
        parser.add_argument("source", type=Path)
        parser.add_argument("decoded", type=Path)
        args = parser.parse_args(sys.argv[2:])
        print(json.dumps(compare(args.source, args.decoded), indent=2, allow_nan=False))
        return
    parser = argparse.ArgumentParser()
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("bin_dir", type=Path)
    parser.add_argument("--single", choices=MATRIX)
    parser.add_argument("--regression-reference", type=Path)
    args = parser.parse_args()
    root, binaries = args.output_dir.resolve(), args.bin_dir.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if args.regression_reference:
        result = regression(root, binaries, args.regression_reference.resolve())
        write_json(root / "regression.json", result)
        print(result["status"])
        return
    results = []
    for name in [args.single] if args.single else MATRIX:
        result = case(root, binaries, name)
        results.append(result)
        write_json(root / "results.json", {"status": "PASS_STRUCTURAL" if all(r["status"] == "PASS_STRUCTURAL" for r in results) else "FAIL", "cases": results,
                   "amplitude_scope": "Record finite peaks, over-unity counts, RMS and silent-region energy; legal near-full-scale lossy overshoot is not a structural format failure",
                   "resource_scope": "Linux wait4 on each timeout process tree; 2GiB RLIMIT_AS applies per process virtual address space; RSS includes brief harness inheritance; concurrent builds prevent performance ranking"})
        print(json.dumps({key: result[key] for key in ["name", "frames", "status"]}), flush=True)
        if result["status"] != "PASS_STRUCTURAL":
            break
    boundary = root / "over-limit"
    boundary.mkdir(parents=True, exist_ok=True)
    rejected = bounded([binaries / "adapter", boundary, 48000 * 600 + 1, 5], boundary, "encode", 30)
    rejected["status"] = "PASS_REJECTED" if rejected["exit_code"] == 1 and "input exceeds the 28800000-frame experimental limit" in (boundary / "encode.stderr").read_text() and not (boundary / "input.f32le").exists() and not (boundary / "encoded.ogg").exists() else "FAIL"
    write_json(root / "over-limit.json", rejected)
    summary = json.loads((root / "results.json").read_text())
    summary["maximum_length_boundary"] = rejected
    if rejected["status"] != "PASS_REJECTED":
        summary["status"] = "FAIL_LENGTH_LIMIT"
    write_json(root / "results.json", summary)


if __name__ == "__main__":
    main()
