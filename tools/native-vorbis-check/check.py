#!/usr/bin/env python3
"""Software evidence for the real media adapter; never play or admit by listening"""
import argparse
import importlib.util
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import traceback

import numpy as np

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
PROBE = REPO / "tools/canonical-audio-probe"
spec = importlib.util.spec_from_file_location("native_vorbis_metrics", PROBE / "fullband-regression/probe.py")
metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(metrics)
digest, measure, ogg, run, save, wrap_pcm = (getattr(metrics, name) for name in
    ("digest", "measure", "ogg", "run", "save", "wrap_pcm"))

RATES = [1, 7, 8000, 11025, 16000, 22050, 32000, 44100, 47999, 48000,
         48001, 64000, 88200, 96000, 176400, 191999, 192000]


def wav(path, rate, frames):
    pcm = np.asarray(frames, dtype="<f4")
    assert pcm.ndim == 2 and pcm.shape[1] == 2 and len(pcm) > 0
    size = pcm.nbytes
    with path.open("xb") as output:
        output.write(b"RIFF" + struct.pack("<I", 36 + size) + b"WAVEfmt "
                     + struct.pack("<IHHIIHH", 16, 3, 2, rate, rate * 8, 8, 32)
                     + b"data" + struct.pack("<I", size))
        pcm.tofile(output)


def old_generator(root):
    # Reuse the tracked old input recipe verbatim, not the failed old encoder
    path = PROBE / "rusty-vorbis/src/bin/adapter.rs"
    text = path.read_text()
    start = text.index("        let pcm: Vec<f32> =")
    end = text.index("        for value in &pcm", start)
    recipe = text[start:end]
    source = root / "input-generator.rs"
    source.write_text('use std::{fs::OpenOptions, io::{BufWriter,Write}};\n'
        'fn main() { let args:Vec<_>=std::env::args().collect();\n'
        'let frames:usize=args[2].parse().unwrap(); let signal=args[3].as_str();\n'
        'let pulse=signal=="pulse"; let mut writer=BufWriter::new(OpenOptions::new()'
        '.write(true).create_new(true).open(&args[1]).unwrap());\n'
        'for start in (0..frames).step_by(1000) {\n' + recipe
        + 'for value in pcm {writer.write_all(&value.to_le_bytes()).unwrap();}}\n'
        'writer.flush().unwrap();}\n')
    binary = root / "input-generator"
    command = ["rustc", "--edition", "2024", "-O", str(source), "-o", str(binary)]
    with (root / "input-generator-build.log").open("x") as log:
        subprocess.run(command, check=True, stdout=log, stderr=subprocess.STDOUT)
    save(root / "input-generator-build.json", {"command": command,
         "tracked_recipe_sha256": digest(path), "generated_source_sha256": digest(source),
         "binary_sha256": digest(binary)})
    return binary


def historical_sources(root, driver, lab):
    old = json.loads((REPO / "testdata/synthetic/canonical-audio-probe/fullband-observations-20261003.json").read_text())
    generator = old_generator(root)
    inputs = root / "inputs"
    inputs.mkdir()
    dev = root / "dev"
    generated = run([lab, "generate-dev", dev], root / "generate-dev", resources=True)
    assert generated["exit_code"] == 0
    dev_wav = dev / "cocobeat-64.wav"
    assert digest(dev_wav) == "3390dd080cb536fd4a598ea933618dd99b4bf697c0b2874ff5cb0e220feb09e8"
    raw = inputs / "dev-song-64s.f32le"
    assert run([driver, "resample", dev_wav, raw], root / "dev-resample")["exit_code"] == 0
    recipes = {"long-600s": (28800000, "tail"), "short-1": (1, "tail"),
               "short-1024": (1024, "tail"), "short-1025": (1025, "tail"),
               "short-48128": (48128, "tail"), "near-full": (48000, "near-full"),
               "pulse-1s": (48000, "pulse"), "silence-1s": (48000, "silence"),
               "edge-silence-3s": (144000, "edge-silence")}
    rows, missing = [], []
    for item in old["cases"]:
        name = item["name"]
        source = REPO / item["source"]
        raw_source = not name.startswith("resample-")
        if name in recipes:
            source = inputs / (name + ".f32le")
            frames, signal = recipes[name]
            assert run([generator, source, frames, signal], root / (name + "-generate"))["exit_code"] == 0
        elif name == "dev-song-64s":
            source = raw
        elif name == "head-tail-impulses":
            source = inputs / (name + ".f32le")
            pcm = np.zeros((48000, 2), dtype="<f4")
            pcm[0], pcm[-1] = [0.75, -0.5], [-0.5, 0.75]
            pcm.tofile(source)
        if not source.exists():
            missing.append({"name": name, "status": "NOT RUN", "reason": "historical source absent and complete tracked recipe unavailable",
                            "expected_sha256": item["source_sha256"]})
            continue
        assert digest(source) == item["source_sha256"], f"Historical source drift: {name}"
        wrapped = inputs / (name + ".wav")
        if raw_source:
            wrap_pcm(source, wrapped)
        else:
            shutil.copyfile(source, wrapped)
        rows.append({"name": name, "source": wrapped, "origin": "historical byte-identical",
                     "old_source_sha256": item["source_sha256"], "snr_min_db": 35 if name == "near-full" else None})
    return rows, missing, raw


def additional_sources(root, dev_raw):
    rows = []
    inputs = root / "inputs"
    time = np.arange(48000, dtype=np.float64) / 48000
    left, right = np.sin(2 * np.pi * 440 * time), np.sin(2 * np.pi * 1000 * time)
    for name, pcm in [("new-quiet", np.column_stack((left, right)) * 0.01),
                      ("new-swapped", np.column_stack((right, left)) * 0.99),
                      ("new-correlated", np.column_stack((left, left)) * 0.99),
                      ("new-opposed", np.column_stack((left, -left)) * 0.99),
                      ("new-domain-boundary", np.column_stack((left, right)) * 4),
                      ("new-domain-positive-edge", np.tile([4., -4.], (1025, 1))),
                      ("new-domain-outside", np.tile([np.nextafter(np.float32(4), np.float32(np.inf)), 0.], (1025, 1))),
                      ("new-domain-negative-outside", np.tile([-np.nextafter(np.float32(4), np.float32(np.inf)), 0.], (1025, 1))),
                      ("new-nonfinite", np.tile([np.inf, 0.], (1025, 1)))]:
        source = inputs / (name + ".wav")
        wav(source, 48000, pcm)
        rows.append({"name": name, "source": source, "origin": "new declared control",
                     "reject": "outside" in name or "nonfinite" in name,
                     "snr_min_db": 35 if name in ("new-quiet", "new-swapped", "new-correlated", "new-opposed") else None})
    for rate in RATES:
        length = max(1, rate // 10)
        pcm = np.zeros((length, 2), dtype="<f4")
        pcm[:, 0] = np.where(np.arange(length) < length // 2, -32700 / 32768, 32700 / 32768)
        pcm[:, 1] = -pcm[:, 0]
        source = inputs / f"new-resample-{rate}.wav"
        wav(source, rate, pcm)
        rows.append({"name": f"new-resample-{rate}", "source": source,
                     "origin": "new normalized step, anti-phase stereo", "source_rate": rate,
                     "snr_min_db": None})
    # Ten minutes of non-silent original music, preserving its full 64-second recipe per cycle
    source = inputs / "new-original-music-600s.f32le"
    with source.open("xb") as output, dev_raw.open("rb") as original:
        remaining = 600 * 48000 * 8
        while remaining:
            original.seek(0)
            block = original.read(min(remaining, dev_raw.stat().st_size))
            output.write(block)
            remaining -= len(block)
    wrapped = inputs / "new-original-music-600s.wav"
    wrap_pcm(source, wrapped)
    rows.append({"name": "new-original-music-600s", "source": wrapped,
                 "origin": "new exact repetition of original 64-second PCM, cut at 600s", "snr_min_db": None})
    return rows


def execute(root, item, driver):
    case = root / "cases" / item["name"]
    case.mkdir()
    source = item["source"]
    row = {**item, "source": str(source.relative_to(root)), "source_sha256": digest(source)}
    output = case / "canonical.ogg"
    row["encode"] = run([driver, "encode", source, output], case / "encode", seconds=60, resources=True)
    if item.get("reject"):
        error = (case / "encode.stderr").read_text()
        row["error"] = error
        expected = "unsupported input domain" if "outside" in item["name"] else "non-finite"
        row["status"] = "PASS_REJECTION" if row["encode"]["exit_code"] == 1 and expected in error and not output.exists() else "FAIL_REJECTION"
        return row
    if row["encode"]["exit_code"] != 0:
        row["status"] = "FAIL_ENCODE_OR_RESOURCE"
        return row
    row["metadata"] = json.loads((case / "encode.stdout").read_text())
    n = row["metadata"]["frames"]
    pcm = {key: case / f"{key}.f32le" for key in ("input", "strict", "reference", "ffmpeg")}
    for key, command in [("input", [driver, "resample", source, pcm["input"]]),
                         ("strict", [driver, "readback", output, n, pcm["strict"]]),
                         ("reference", [driver, "reference", output, pcm["reference"]]),
                         ("ffmpeg", ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "warning", "-xerror", "-err_detect", "explode", "-c:a", "vorbis", "-i", output, "-map", "0:a:0", "-vn", "-c:a", "pcm_f32le", "-f", "f32le", "-n", pcm["ffmpeg"]])]:
        row[key] = run(command, case / key, seconds=60, resources=True)
    row["resample"] = json.loads((case / "input.stdout").read_text())
    row["ogg"] = ogg(output)
    checks = {"complete_readers": all(row[key]["exit_code"] == 0 and pcm[key].stat().st_size == n * 8 for key in ("input", "strict", "reference")),
              "eos": row["ogg"]["eos_granules"] == [n], "format": row["ogg"]["channels"] == 2 and row["ogg"]["sample_rate"] == 48000,
              "source_unchanged": digest(source) == row["source_sha256"]}
    if checks["complete_readers"]:
        a, b = (np.memmap(pcm[key], dtype="<f4", mode="r") for key in ("strict", "reference"))
        difference = max(float(np.max(abs(a[start:start + 96000] - b[start:start + 96000]))) for start in range(0, len(a), 96000))
        checks["dual_finite"] = bool(np.isfinite(a).all() and np.isfinite(b).all())
        tolerance = 1e-6 * max(1.0, float(np.max(abs(a))), float(np.max(abs(b))))
        checks["dual_agreement"] = difference <= tolerance
        row["dual_max_abs_difference"] = difference
        row["dual_absolute_tolerance"] = tolerance
        if checks["dual_finite"]:
            row["metrics"] = measure(pcm["input"], pcm["strict"])
            minimum = item.get("snr_min_db")
            if minimum is not None:
                checks["preset_tone_snr"] = all(value is not None and value >= minimum for value in row["metrics"]["whole"]["snr_db"])
    row["checks"] = checks
    row["ffmpeg"]["actual_frames"] = pcm["ffmpeg"].stat().st_size // 8 if pcm["ffmpeg"].exists() else None
    row["ffmpeg"]["status"] = "PASS_COMPLETE" if row["ffmpeg"]["exit_code"] == 0 and row["ffmpeg"]["actual_frames"] == n else "FAIL_COMPLETE"
    row["status"] = "PASS_SOFTWARE_CASE" if all(checks.values()) else "FAIL_SOFTWARE_CASE"
    row["sha256"] = {path.name: digest(path) for path in [output, *pcm.values()] if path.exists()}
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("driver", type=Path)
    parser.add_argument("lab", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir()
    (root / "cases").mkdir()
    driver, lab = args.driver.resolve(), args.lab.resolve()
    items, missing, dev_raw = historical_sources(root, driver, lab)
    items.extend(additional_sources(root, dev_raw))
    rows = []
    for item in items:
        try:
            row = execute(root, item, driver)
        except Exception:
            row = {"name": item["name"], "status": "FAIL_DIAGNOSTIC", "error": traceback.format_exc()}
        rows.append(row)
        save(root / "cases" / item["name"] / "result.json", row)
        print(json.dumps({"name": row["name"], "status": row["status"]}), flush=True)
    passed = all(row["status"].startswith("PASS") for row in rows)
    save(root / "summary.json", {"schema": 1, "status": "PASS_SOFTWARE_MATRIX" if passed else "FAIL_SOFTWARE_MATRIX",
         "driver_sha256": digest(driver), "lab_sha256": digest(lab),
         "source_hashes": {str(path.relative_to(REPO)): digest(path) for path in [Path(__file__).resolve(), HERE / "driver.rs", PROBE / "fullband-regression/probe.py", PROBE / "oxideav/probe.py", REPO / "crates/cocobeat-media/src/encode.rs"]},
         "resample_rates_measured": RATES, "historical_missing": missing, "cases": rows,
         "scope": "Software case matrix and two complete independent decoders, not listening acceptance or all-integer-rate peak proof",
         "not_run": ["human listening", "hardware playback", "cross-platform execution by this Linux harness"]})
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
