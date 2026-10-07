#!/usr/bin/env python3
"""Portable software QA for the media examples; no playback or hardware claims"""
import argparse
from array import array
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import struct
import subprocess
import sys
import time
import traceback

REPO = Path(__file__).resolve().parents[2]
CASE_SECONDS = 600
CASES = [
    ("short-1", 48000, 1, None),
    ("short-1024", 48000, 1024, None),
    ("short-1025", 48000, 1025, None),
    ("original-48000", 48000, 48000, None),
    ("original-64s", 48000, 3072000, None),
    ("original-600s", 48000, 28800000, None),
    ("head-tail-impulses", 48000, 48000, None),
    ("silence-48000", 48000, 48000, None),
    ("domain-edge", 48000, 1025, None),
    ("positive-next-up", 48000, 1025, "unsupported input domain"),
    ("negative-next-up", 48000, 1025, "unsupported input domain"),
    ("infinity", 48000, 1025, "non-finite"),
    ("nan", 48000, 1025, "non-finite"),
    ("source-44100", 44100, 44100, None),
]


def save(path, value):
    with path.open("x", encoding="utf-8") as output:
        json.dump(value, output, ensure_ascii=False, indent=2, allow_nan=False)
        output.write("\n")


def digest(path, offset=0):
    result = hashlib.sha256()
    with path.open("rb") as source:
        source.seek(offset)
        for block in iter(lambda: source.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def remaining(deadline):
    seconds = deadline - time.monotonic()
    require(seconds > 0, "Case exceeded its preset 600-second deadline")
    return seconds


def run(command, prefix, deadline, expected_exit=0):
    command = [str(item) for item in command]
    started = time.monotonic()
    row = {"command": command, "cwd": str(REPO), "exit_code": None}
    stdout, stderr = prefix.with_suffix(".stdout"), prefix.with_suffix(".stderr")
    try:
        with stdout.open("xb") as out, stderr.open("xb") as err:
            result = subprocess.run(command, cwd=REPO, stdout=out, stderr=err,
                                    timeout=remaining(deadline), check=False)
        row["exit_code"] = result.returncode
    except subprocess.TimeoutExpired:
        row["timeout"] = True
        raise
    finally:
        row["elapsed_seconds"] = time.monotonic() - started
        row["logs"] = {path.name: digest(path) for path in (stdout, stderr) if path.exists()}
        save(prefix.with_suffix(".json"), row)
    require(row["exit_code"] == expected_exit,
            f"{prefix.name}: exit {row['exit_code']}, expected {expected_exit}; see {stderr}")
    return json.loads(stdout.read_text(encoding="utf-8")) if expected_exit == 0 else None


def ogg_summary(path):
    # The production strict decoder validates structure and CRC; this records actual EOS bytes
    pages, eos, serials, identification, last_flags = 0, [], set(), None, 0
    with path.open("rb") as source:
        while header := source.read(27):
            require(len(header) == 27 and header[:5] == b"OggS\0", "Invalid Ogg page header")
            laces = source.read(header[26])
            require(len(laces) == header[26], "Truncated Ogg lacing")
            body = source.read(sum(laces))
            require(len(body) == sum(laces), "Truncated Ogg page body")
            if pages == 0:
                require(body[:7] == b"\x01vorbis" and len(body) >= 30,
                        "Missing first Vorbis identification header")
                identification = {"channels": body[11], "sample_rate": struct.unpack_from("<I", body, 12)[0]}
            last_flags = header[5]
            serials.add(struct.unpack_from("<I", header, 14)[0])
            if last_flags & 4:
                eos.append(struct.unpack_from("<Q", header, 6)[0])
            pages += 1
    return {**(identification or {}), "pages": pages, "serials": sorted(serials),
            "eos_granules": eos, "last_page_eos": bool(last_flags & 4)}


def compare_pcm(paths, frames, deadline):
    for path in paths:
        require(path.stat().st_size == frames * 8, f"Incomplete PCM: {path.name}")
    peak = [0.0, 0.0, 0.0]
    difference = 0.0
    with paths[0].open("rb") as source, paths[1].open("rb") as strict, paths[2].open("rb") as reference:
        while raw := source.read(65536 * 8):
            remaining(deadline)
            blocks = [raw, strict.read(len(raw)), reference.read(len(raw))]
            samples = []
            for index, block in enumerate(blocks):
                require(len(block) == len(raw), "PCM changed while comparing")
                values = array("f")
                require(values.itemsize == 4, "QA requires 32-bit native float arrays")
                values.frombytes(block)
                if sys.byteorder != "little":
                    values.byteswap()
                require(all(map(math.isfinite, values)), f"Non-finite PCM: {paths[index].name}")
                peak[index] = max(peak[index], max(map(abs, values)))
                samples.append(values)
            difference = max(difference, max(abs(a - b) for a, b in zip(samples[1], samples[2])))
    tolerance = 1e-6 * max(1.0, peak[1], peak[2])
    require(difference <= tolerance, f"Same-position decoder difference {difference} exceeds {tolerance}")
    return {"frames": frames, "finite": True, "peak_source_strict_reference": peak,
            "max_abs_difference": difference, "absolute_tolerance": tolerance,
            "comparison": "same sample positions, no alignment, gain fitting or normalization"}


def check_case(root, fixture, driver, case):
    name, rate, length, rejection = case
    directory = root / name
    directory.mkdir()
    started = time.monotonic()
    deadline = started + CASE_SECONDS
    row = {"name": name, "status": "FAIL", "source_rate": rate, "source_frames": length}
    source, output = directory / "source.wav", directory / "canonical.ogg"
    try:
        row["fixture"] = run([fixture, name, source], directory / "fixture", deadline)
        require(row["fixture"]["case"] == name and row["fixture"]["source_rate"] == rate
                and row["fixture"]["source_frames"] == length, "Unexpected fixture metadata")
        require(source.stat().st_size == 44 + length * 8, "Unexpected fixture WAV size")
        row["source_sha256"] = digest(source)
        with source.open("rb") as wav:
            header = wav.read(44)
        require(header[:4] == b"RIFF" and header[8:16] == b"WAVEfmt "
                and struct.unpack_from("<IHHIIHH", header, 16) == (16, 3, 2, rate, rate * 8, 8, 32)
                and header[36:40] == b"data" and struct.unpack_from("<I", header, 40)[0] == length * 8,
                "Fixture is not the declared F32LE stereo WAV")
        encoded = run([driver, "encode", source, output], directory / "encode", deadline,
                      expected_exit=1 if rejection else 0)
        if rejection:
            error = (directory / "encode.stderr").read_text(encoding="utf-8")
            require(rejection in error and not output.exists(), "Expected rejection/cleanup was not observed")
            row["rejection"] = error.strip()
        else:
            frames = (length * 48000 + rate - 1) // rate
            require(encoded == {"source_rate": rate, "source_frames": length, "frames": frames},
                    "Encode did not report the complete real resampled length")
            row["encoded"] = encoded
            pcm = [directory / f"{kind}.f32le" for kind in ("resampled", "strict", "reference")]
            resampled = run([driver, "resample", source, pcm[0]], directory / "resample", deadline)
            require(all(resampled[key] == value for key, value in encoded.items()), "Resample metadata differs")
            for kind, command in [("strict", [driver, "readback", output, frames, pcm[1]]),
                                  ("reference", [driver, "reference", output, pcm[2]])]:
                decoded = run(command, directory / kind, deadline)
                require(decoded == {"frames": frames, "channels": 2, "sample_rate": 48000},
                        f"{kind} did not decode the complete canonical stream")
            row["ogg"] = ogg_summary(output)
            require(row["ogg"]["channels"] == 2 and row["ogg"]["sample_rate"] == 48000
                    and row["ogg"]["eos_granules"] == [frames] and row["ogg"]["last_page_eos"]
                    and len(row["ogg"]["serials"]) == 1, "Canonical format/actual EOS mismatch")
            row["pcm"] = compare_pcm(pcm, frames, deadline)
            require((row["pcm"]["peak_source_strict_reference"][0] == 0) == (name == "silence-48000"),
                    "Declared silent/non-silent fixture differs from actual resampled PCM")
            if rate == 48000:
                require(digest(source, 44) == digest(pcm[0]), "Same-rate source samples changed")
            row["ogg_sha256"] = digest(output)
            run([driver, "encode", source, output], directory / "existing-output", deadline, expected_exit=1)
            require(digest(output) == row["ogg_sha256"], "Existing output changed after rejected replacement")
            row["existing_output_unchanged"] = True
            row["pcm_sha256"] = {path.name: digest(path) for path in pcm}
        require(digest(source) == row["source_sha256"], "Original source changed")
        remaining(deadline)
        row["source_unchanged"] = True
        # Owned large PCM is removed only after successful comparison and recorded hashes
        removed = [source] + ([] if rejection else pcm)
        row["removed_after_verification"] = [path.name for path in removed]
        for path in removed:
            path.unlink()
        row["status"] = "PASS_REJECTION" if rejection else "PASS_SOFTWARE_CASE"
    except Exception:
        row["error"] = traceback.format_exc()
    row["elapsed_seconds"] = time.monotonic() - started
    save(directory / "result.json", row)
    print(f"{name}: {row['status']} ({row['elapsed_seconds']:.2f}s)", flush=True)
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("examples", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root, examples = args.output.resolve(), args.examples.resolve()
    suffix = ".exe" if os.name == "nt" else ""
    fixture, driver = [examples / (name + suffix) for name in ("native-vorbis-fixture", "native-vorbis-check")]
    require(fixture.is_file() and driver.is_file(), "Build both media examples before running native QA")
    root.mkdir(parents=True, exist_ok=False)
    sources = ["Cargo.toml", "Cargo.lock", "crates/cocobeat-media/Cargo.toml",
               "crates/cocobeat-runtime/src/dev_song.rs", ".github/workflows/media-candidate.yml",
               "tools/native-vorbis-check/driver.rs", "tools/native-vorbis-check/fixture.rs",
               "tools/native-vorbis-check/native.py"]
    sources += [str(path.relative_to(REPO)) for path in sorted((REPO / "crates/cocobeat-media/src").rglob("*.rs"))]
    save(root / "provenance.json", {
        "github_sha": os.environ.get("GITHUB_SHA"), "github_target": os.environ.get("RUSTUP_TOOLCHAIN"),
        "platform": platform.platform(), "machine": platform.machine(), "python": sys.version,
        "command": [sys.executable, *sys.argv], "case_timeout_seconds": CASE_SECONDS,
        "binaries_sha256": {str(path): digest(path) for path in (fixture, driver)},
        "sources_sha256": {path: digest(REPO / path) for path in sources},
        "asset_license": "CC0-1.0", "asset_creator": "CoCoBeat contributors with OpenAI Codex assistance",
        "origin": "Original dev_song integer PCM and declared QA controls; encoded Ogg derives only from these fixtures",
        "generator_code_license": "MPL-2.0",
        "not_run": ["hardware/audio playback", "listening acceptance", "SNR/admission matrix"],
    })
    rows = [check_case(root, fixture, driver, case) for case in CASES]
    passed = all(row["status"].startswith("PASS_") for row in rows)
    save(root / "summary.json", {"status": "PASS" if passed else "FAIL", "cases": rows})
    return 0 if passed else 1


if __name__ == "__main__":
    sys.exit(main())
