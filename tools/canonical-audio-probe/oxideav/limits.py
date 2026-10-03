#!/usr/bin/env python3
"""Bounded OxideAV long-input probe, reusing the existing streaming measurements"""
import argparse
import copy
import json
import math
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "rusty-vorbis"))
from limits import bounded, compare, difference, digest, write_json
from probe import ogg

MAX_FRAMES = 28_800_000


def page_checks(stream, frames):
    pages = stream["pages"]
    granules = [page["granule"] for page in pages if page["granule"] != 2**64 - 1]
    return {
        "single_serial": len({page["serial"] for page in pages}) == 1,
        "page_sequence": [page["sequence"] for page in pages] == list(range(len(pages))),
        "bos_only_first": [i for i, page in enumerate(pages) if page["flags"] & 2] == [0],
        "eos_only_last": [i for i, page in enumerate(pages) if page["flags"] & 4] == [len(pages)-1],
        "known_flags": all(page["flags"] & ~7 == 0 for page in pages),
        "monotonic_granules": all(a <= b for a, b in zip(granules, granules[1:])),
        "exact_eos": stream["eos_granules"] == [frames],
        "canonical_format": stream["sample_rate"] == 48_000 and stream["channels"] == 2,
    }


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--self-check":
        stream = ogg(Path(sys.argv[2]))
        frames = int(sys.argv[3])
        assert all(page_checks(stream, frames).values())
        for check, index, field, value in [
            ("page_sequence", -1, "sequence", 999),
            ("single_serial", -1, "serial", stream["pages"][0]["serial"] ^ 1),
            ("monotonic_granules", 1, "granule", frames + 1),
            ("eos_only_last", 0, "flags", 6),
        ]:
            corrupted = copy.deepcopy(stream)
            corrupted["pages"][index][field] = value
            assert not page_checks(corrupted, frames)[check], check
        assert not page_checks(stream, frames + 1)["exact_eos"]
        print("PASS: valid Ogg accepted and five corrupted page contracts rejected")
        return
    parser = argparse.ArgumentParser()
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("bin_dir", type=Path)
    source = parser.add_mutually_exclusive_group()
    source.add_argument("--frames", type=int, default=MAX_FRAMES)
    source.add_argument("--input", type=Path)
    parser.add_argument("--quality", type=float, default=0.5)
    parser.add_argument("--encode-seconds", type=int, default=3600)
    args = parser.parse_args()
    if not 1 <= args.frames <= MAX_FRAMES or not math.isfinite(args.quality) or not 0 <= args.quality <= 1 or args.encode_seconds <= 0:
        parser.error("positive bounded frame count, quality in [0,1], and positive timeout required")
    root, binaries = args.output_dir.resolve(), args.bin_dir.resolve()
    frames = args.frames
    if args.input:
        args.input = args.input.resolve()
        size = args.input.stat().st_size
        if not size or size % 8 or size // 8 > MAX_FRAMES:
            parser.error("input must contain 1..28800000 complete stereo F32LE frames")
        frames = size // 8
    root.mkdir(parents=True, exist_ok=False)
    sources = [Path(__file__), Path(__file__).parent / "src/main.rs", Path(__file__).parent / "Cargo.toml",
               Path(__file__).parent / "Cargo.lock", Path(__file__).parent / "probe.py",
               Path(__file__).parent.parent / "readback.rs", Path(__file__).parent.parent / "rusty-vorbis/limits.py",
               Path(__file__).parent.parent / "rusty-vorbis/run.py"]
    result = {
        "status": "RUNNING", "pid": os.getpid(), "frames": frames, "quality": args.quality,
        "source_kind": "external_f32le" if args.input else "synthetic_tail",
        "repo_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "source_sha256": {str(path.resolve()): digest(path) for path in sources},
        "binary_sha256": {name: digest(binaries / name) for name in ["cocobeat-oxideav-probe", "readback"]},
        "scope": "Linux file encoding and independent full readback only; no playback or production admission",
        "resource_scope": "Per-process 2GiB RLIMIT_AS; wait4 RSS includes brief harness inheritance; no throughput ranking",
    }
    if args.input:
        result["external_input"] = {"path": str(args.input), "sha256": digest(args.input), "bytes": frames * 8}

    def record(phase):
        result["phase"] = phase
        write_json(root / "result.json", result)
        print(json.dumps({"phase": phase, "status": result["status"], "pid": result["pid"]}), flush=True)

    try:
        record("boundary")
        boundary = root / "over-limit"
        boundary.mkdir()
        result["boundary"] = bounded([binaries / "cocobeat-oxideav-probe", boundary / "output", MAX_FRAMES + 1, args.quality], boundary, "encode", 30)
        result["boundary"]["rejected_before_output"] = result["boundary"]["exit_code"] != 0 and "input exceeds the 28800000-frame experimental limit" in (boundary / "encode.stderr").read_text() and not (boundary / "output").exists()
        if not result["boundary"]["rejected_before_output"]:
            raise ValueError("frame limit did not reject before output creation")
        record("encode")
        case = root / "case"
        case.mkdir()
        source_args = ["--input", args.input, args.quality] if args.input else [frames, args.quality]
        result["encode"] = bounded([binaries / "cocobeat-oxideav-probe", case, *source_args], case, "encode", args.encode_seconds)
        if result["encode"]["exit_code"] != 0:
            result["status"] = "FAIL_ENCODE"
            result["error"] = (case / "encode.stderr").read_text()
            return
        record("container")
        stream = ogg(case / "encoded.ogg")
        result["ogg"] = stream
        result["checks"] = page_checks(stream, frames)
        commands = {
            "symphonia": [binaries / "readback", case / "encoded.ogg", case / "symphonia.f32le"],
            "ffmpeg": ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "warning", "-xerror", "-err_detect", "explode", "-c:a", "vorbis", "-i", case / "encoded.ogg", "-map", "0:a:0", "-vn", "-c:a", "pcm_f32le", "-f", "f32le", "-y", case / "ffmpeg.f32le"],
        }
        for decoder, command in commands.items():
            record(decoder)
            result[decoder] = bounded(command, case, decoder, 600)
            if result[decoder]["exit_code"] != 0:
                raise ValueError(f"{decoder} readback failed")
            if decoder == "symphonia":
                metadata = json.loads((case / "symphonia.stdout").read_text())
                result[decoder]["metadata"] = metadata
                result["checks"]["decoded_format"] = metadata == {"sample_rate": 48_000, "channels": 2, "frames": frames}
            record(decoder + "_compare")
            result[decoder]["comparison"] = compare(case / "input.f32le", case / f"{decoder}.f32le")
            result["checks"][decoder + "_finite_exact_frames"] = result[decoder]["comparison"]["frames"] == frames
        record("decoder_agreement")
        result["decoder_agreement"] = difference(case / "symphonia.f32le", case / "ffmpeg.f32le")
        result["checks"]["decoder_agreement"] = result["decoder_agreement"]["same_byte_count"] and result["decoder_agreement"]["nonfinite_pairs"] == 0 and result["decoder_agreement"]["peak_difference"] <= 1e-5
        result["artifact_sha256"] = {name: digest(case / name) for name in ["input.f32le", "encoded.ogg", "symphonia.f32le", "ffmpeg.f32le"]}
        result["status"] = "PASS_STRUCTURAL" if all(result["checks"].values()) else "FAIL_STRUCTURAL"
    except Exception as error:
        result["status"] = "FAIL"
        result["error"] = f"{type(error).__name__}: {error}"
    finally:
        record("completed")
        if result["status"] != "PASS_STRUCTURAL":
            raise SystemExit(1)


if __name__ == "__main__":
    main()
