#!/usr/bin/env python3
"""Fixed same-source fullband regression, with no codec admission or parameter search"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tarfile
import traceback

import numpy as np

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
sys.path.insert(0, str(HERE.parent / "oxideav"))
from probe import ogg, run

REVISION = "e1b3a261f1e4866dc377680f846225e75cf9b670"
CANDIDATE = Path("tools/canonical-audio-probe/rusty-candidate")
TRANSIENT = REPO / "target/codec-transient-20261003"
OLD = REPO / "target/rusty-candidate-q10-20261003"
SETUP = "vendor/rusty_vorbis/src/setup_q4_stereo.bin"


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def load(path):
    return json.loads(path.read_text())


def wrap_pcm(source, wav):
    size = source.stat().st_size
    assert size % 8 == 0 and size > 0
    with wav.open("xb") as out, source.open("rb") as pcm:
        out.write(b"RIFF" + struct.pack("<I", 36 + size) + b"WAVEfmt "
                  + struct.pack("<IHHIIHH", 16, 3, 2, 48000, 384000, 8, 32)
                  + b"data" + struct.pack("<I", size))
        shutil.copyfileobj(pcm, out)


def snr(signal, error):
    return [10 * math.log10(s / e) if s > 0 and e > 0 else None
            for s, e in zip(signal, error)]


def statistics(x, y):
    assert x.shape == y.shape and x.ndim == 2 and x.shape[1] == 2 and len(x) > 0
    assert np.isfinite(x).all() and np.isfinite(y).all()
    signal = np.sum(x * x, axis=0)
    energy = np.sum(y * y, axis=0)
    error = np.sum((y - x) ** 2, axis=0)
    return {"frames": len(x), "signal_energy": signal.tolist(), "decoded_energy": energy.tolist(),
            "error_energy": error.tolist(), "snr_db": snr(signal, error),
            "rmse": np.sqrt(error / len(x)).tolist(),
            "max_abs_error": np.max(abs(y - x), axis=0).tolist(),
            "source_peak": np.max(abs(x), axis=0).tolist(), "decoded_peak": np.max(abs(y), axis=0).tolist(),
            "source_over_unity": np.sum(abs(x) > 1, axis=0).tolist(),
            "decoded_over_unity": np.sum(abs(y) > 1, axis=0).tolist()}


def bands(values):
    n = len(values)
    spectrum = np.fft.rfft(values, axis=0)
    weights = np.ones(len(spectrum)) * 2
    weights[0] = 1
    if n % 2 == 0:
        weights[-1] = 1
    power = abs(spectrum) ** 2 * weights[:, None] / n
    high = np.fft.rfftfreq(n, 1 / 48000) >= 20625
    assert np.allclose(np.sum(power, axis=0), np.sum(values * values, axis=0), atol=1e-10)
    return np.array([np.sum(power[~high], axis=0), np.sum(power[high], axis=0)])


def measure(source, actual):
    assert source.stat().st_size == actual.stat().st_size
    x = np.memmap(source, dtype="<f4", mode="r").reshape(-1, 2)
    y = np.memmap(actual, dtype="<f4", mode="r").reshape(-1, 2)
    blocks, spectrum = [], {key: np.zeros((2, 2)) for key in ("source", "decoded", "error")}
    for start in range(0, len(x), 48000):
        a, b = x[start:start + 48000].astype(np.float64), y[start:start + 48000].astype(np.float64)
        item = statistics(a, b)
        item["start_frame"] = start
        blocks.append(item)
        for key, values in [("source", a), ("decoded", b), ("error", b - a)]:
            spectrum[key] += bands(values)
    whole = {key: np.sum([b[key] for b in blocks], axis=0).tolist()
             for key in ("signal_energy", "decoded_energy", "error_energy", "source_over_unity", "decoded_over_unity")}
    whole.update({key: np.max([b[key] for b in blocks], axis=0).tolist()
                  for key in ("max_abs_error", "source_peak", "decoded_peak")})
    whole.update({"frames": len(x), "snr_db": snr(whole["signal_energy"], whole["error_energy"]),
                  "rmse": np.sqrt(np.array(whole["error_energy"]) / len(x)).tolist()})
    windows = {"first_2048": (0, min(2048, len(x))), "last_2048": (max(0, len(x) - 2048), len(x))}
    for channel in range(2):
        peak = int(np.argmax(abs(x[:, channel])))
        if peak:
            windows[f"source_peak_ch{channel}_before"] = (max(0, peak - 2048), peak)
        windows[f"source_peak_ch{channel}_after"] = (peak, min(len(x), peak + 2048))
    return {"whole": whole, "one_second_blocks": blocks,
            "spectral_energy": {key: value.tolist() for key, value in spectrum.items()},
            "regions": {name: {"start_frame": lo, **statistics(x[lo:hi].astype(np.float64), y[lo:hi].astype(np.float64))}
                        for name, (lo, hi) in windows.items()}}


def self_check(root):
    x = np.array([[1., 0.], [-1., 0.]])
    assert statistics(x, x)["snr_db"] == [None, None]
    assert statistics(x, x * 0.5)["snr_db"] == [10 * math.log10(4), None]
    assert bands(x).sum(axis=0).tolist() == [2., 0.]
    try:
        statistics(x, x[:1])
    except AssertionError:
        pass
    else:
        raise AssertionError("partial readback must fail")
    check = root / "harness-check"
    (check / "cases").mkdir(parents=True)
    source = check / "empty.f32le"
    source.touch()
    result = record_case(check, {"name": "malformed-input", "source": source, "raw": True}, {})
    assert result["status"] == "FAIL_DIAGNOSTIC" and "AssertionError" in result["error"]
    assert load(check / "cases/malformed-input/result.json") == result


def build(root):
    frozen = load(TRANSIENT / "FROZEN.json")
    assert digest(TRANSIENT / "FROZEN.json") == "b71111c6d994eabcffbce23924a5beeb0eba196229e73b11f85acf536e0e69cf"
    for name in ["minimal-fullband.patch", "fullband/" + SETUP]:
        assert digest(TRANSIENT / name) == frozen["sha256"][name]
    snapshot = root / "snapshot"
    snapshot.mkdir()
    archive = root / "snapshot.tar"
    with archive.open("xb") as output:
        subprocess.run(["git", "archive", REVISION, "Cargo.toml", "crates/cocobeat-media",
                        "crates/cocobeat-schema", str(CANDIDATE)], cwd=REPO, stdout=output, check=True)
    with tarfile.open(archive) as source:
        source.extractall(snapshot, filter="data")
    manifest = snapshot / "Cargo.toml"
    text, count = re.subn(r"members = \[.*?\]", 'members = ["crates/cocobeat-media", "crates/cocobeat-schema"]', manifest.read_text(), count=1, flags=re.S)
    assert count == 1
    manifest.write_text(text)
    binaries = {}
    for variant in ("baseline", "fullband"):
        project = root / variant
        shutil.copytree(snapshot / CANDIDATE, project)
        setup = project / SETUP
        assert digest(setup) == "8dbb400d1883f8e30fd473a66978e934dfd4029393d187050f82e2069ceef7eb"
        if variant == "fullband":
            data = bytearray(setup.read_bytes())
            assert data[4088:4091] == bytes.fromhex("e00600")
            data[4088:4091] = bytes.fromhex("000800")
            setup.write_bytes(data)
            assert digest(setup) == frozen["sha256"]["fullband/" + SETUP]
        manifest = project / "Cargo.toml"
        manifest.write_text(manifest.read_text().replace('path = "../../../crates/cocobeat-media"',
                            f'path = "{snapshot / "crates/cocobeat-media"}"')
                            + '\n[[bin]]\nname = "fullband-strict-readback"\npath = "src/readback.rs"\n')
        shutil.copyfile(HERE / "readback.rs", project / "src/readback.rs")
        command = ["cargo", "build", "--offline", "--locked", "--release", "-j2", "--manifest-path", manifest,
                   "--target-dir", root / "build"]
        with (root / f"{variant}-build.log").open("w") as log:
            result = subprocess.run([str(v) for v in command], stdout=log, stderr=subprocess.STDOUT, timeout=300)
        save(root / f"{variant}-build.json", {"command": list(map(str, command)), "exit_code": result.returncode})
        result.check_returncode()
        binary = root / f"{variant}-encoder"
        shutil.copy2(root / "build/release/cocobeat-rusty-candidate", binary)
        binaries[variant] = binary
        if variant == "baseline":
            shutil.copy2(root / "build/release/fullband-strict-readback", root / "strict-reader")
    return binaries


def sources(root):
    rows = []
    for prior in load(OLD / "summary.json")["cases"]:
        source = OLD / prior["name"] / "input.f32le"
        assert digest(source) == prior["sha256"]["input.f32le"]
        rows.append({"name": prior["name"], "source": source, "frames": prior["frames"], "raw": True,
                     "prior_ogg_sha256": prior["sha256"]["encoded.ogg"]})
    for name in ("quiet", "swapped", "correlated", "opposed"):
        source = REPO / f"target/rusty-quality-diagnosis/patched-{name}-q10/input.f32le"
        expected = load(source.parent / "result.json")["sha256"]["input.f32le"]
        assert digest(source) == expected
        rows.append({"name": name, "source": source, "frames": 48000, "raw": True})
    pulse = root / "head-tail.f32le"
    pcm = np.zeros((48000, 2), dtype="<f4")
    pcm[0] = [0.75, -0.5]
    pcm[-1] = [-0.5, 0.75]
    pcm.tofile(pulse)
    rows.append({"name": "head-tail-impulses", "source": pulse, "frames": 48000, "raw": True})
    for rate in (44100, 96000):
        source = REPO / f"target/resample-l1-domain-20261003/{rate}-control025/source.wav"
        rows.append({"name": f"resample-{rate}-control", "source": source, "frames": 48000, "raw": False})
        source = REPO / f"target/media-finite-domain-final-20261003/step-{rate}/source.wav"
        rows.append({"name": f"resample-{rate}-overunity", "source": source, "frames": 4800, "raw": False, "reject": True})
    return rows


def execute(root, item, binaries):
    case = root / "cases" / item["name"]
    case.mkdir()
    source = case / "source.wav"
    if item["raw"]:
        wrap_pcm(item["source"], source)
    else:
        shutil.copyfile(item["source"], source)
    row = {**item, "source": str(item["source"].relative_to(REPO)), "source_sha256": digest(item["source"]),
           "wrapped_wav_sha256": digest(source), "variants": {}}
    for name, binary in binaries.items():
        output = case / name
        result = {"encode": run([binary, source, output], case / f"{name}-encode", seconds=30, resources=True)}
        row["variants"][name] = result
        report = load(output / "report.json") if (output / "report.json").exists() else None
        result["report"] = report
        if item.get("reject"):
            result["status"] = "PASS_GUARD_REJECTION" if (result["encode"]["exit_code"] == 1 and report
                and "unsupported input domain" in report["result"].get("error", "")
                and not (output / "canonical.ogg").exists()) else "FAIL_GUARD_REJECTION"
            continue
        if result["encode"]["exit_code"] != 0:
            result["status"] = "FAIL_ENCODE_OR_RESOURCE"
            continue
        strict, ffmpeg = case / f"{name}-strict.f32le", case / f"{name}-ffmpeg.f32le"
        result["strict"] = run([root / "strict-reader", output / "canonical.ogg", item["frames"], strict],
                               case / f"{name}-strict", seconds=30, resources=True)
        result["ffmpeg"] = run(["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "warning", "-xerror", "-err_detect", "explode",
                                "-c:a", "vorbis", "-i", output / "canonical.ogg", "-map", "0:a:0", "-vn",
                                "-c:a", "pcm_f32le", "-f", "f32le", "-n", ffmpeg], case / f"{name}-ffmpeg", seconds=30, resources=True)
        container = ogg(output / "canonical.ogg")
        result["ogg"] = container
        checks = {"decoder_exit": result["strict"]["exit_code"] == result["ffmpeg"]["exit_code"] == 0,
                  "eos": container["eos_granules"] == [item["frames"]],
                  "format": container["channels"] == 2 and container["sample_rate"] == 48000,
                  "frame_counts": all(p.exists() and p.stat().st_size == item["frames"] * 8 for p in
                                      [strict, ffmpeg, output / "resampled.f32le", output / "decoded.f32le"])}
        if all(checks.values()):
            a = np.memmap(strict, dtype="<f4", mode="r")
            b = np.memmap(ffmpeg, dtype="<f4", mode="r")
            difference = max(float(np.max(abs(a[i:i + 96000] - b[i:i + 96000]))) for i in range(0, len(a), 96000))
            result["dual_decoder_max_abs_difference"] = difference
            checks["finite"] = bool(np.isfinite(a).all() and np.isfinite(b).all())
            checks["dual_agreement"] = difference <= 1e-6
            checks["candidate_reader_identical"] = digest(strict) == digest(output / "decoded.f32le")
            if item["raw"]:
                checks["same_encoder_input"] = digest(output / "resampled.f32le") == row["source_sha256"]
            if checks["finite"]:
                result["metrics"] = measure(output / "resampled.f32le", strict)
            if name == "baseline" and "prior_ogg_sha256" in item:
                result["prior_frozen_ogg_identical"] = digest(output / "canonical.ogg") == item["prior_ogg_sha256"]
        result["checks"] = checks
        result["status"] = "PASS_STRUCTURAL" if all(checks.values()) else "FAIL_STRUCTURAL"
    a, b = (row["variants"][v] for v in ("baseline", "fullband"))
    if "metrics" in a and "metrics" in b:
        assert digest(case / "baseline/resampled.f32le") == digest(case / "fullband/resampled.f32le")
        row["snr_delta_db"] = [y - x if x is not None and y is not None else None
                               for x, y in zip(a["metrics"]["whole"]["snr_db"], b["metrics"]["whole"]["snr_db"])]
        row["ogg_byte_delta"] = b["ogg"]["bytes"] - a["ogg"]["bytes"]
    save(case / "result.json", row)
    print(json.dumps({"case": item["name"], "status": [a["status"], b["status"]], "snr_delta_db": row.get("snr_delta_db")}), flush=True)
    return row


def record_case(root, item, binaries):
    try:
        return execute(root, item, binaries)
    except Exception:
        row = {"name": item["name"], "status": "FAIL_DIAGNOSTIC", "error": traceback.format_exc()}
        save(root / "cases" / item["name"] / "result.json", row)
        return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir()
    (root / "cases").mkdir()
    self_check(root)
    for path in HERE.iterdir():
        if path.is_file():
            shutil.copyfile(path, root / path.name)
    removed = {key: os.environ.pop(key) for key in list(os.environ) if key.startswith("VORBIS_")}
    os.environ["VORBIS_Q_SCALE"] = "30"
    save(root / "environment.json", {"removed_inherited_vorbis": removed, "fixed_vorbis": {"VORBIS_Q_SCALE": "30"},
                                    "rustc": subprocess.check_output(["rustc", "-Vv"], text=True),
                                    "ffmpeg": subprocess.check_output(["ffmpeg", "-version"], text=True).splitlines()[0],
                                    "numpy": np.__version__, "revision": REVISION})
    binaries = build(root)
    inputs = sources(root)
    save(root / "inputs.json", [{**item, "source": str(item["source"].relative_to(REPO)),
                                 "sha256": digest(item["source"])} for item in inputs])
    rows = [record_case(root, item, binaries) for item in inputs]
    passed = all("variants" in row and all(result["status"].startswith("PASS_")
                 for result in row["variants"].values()) for row in rows)
    save(root / "results.json", {"status": "PASS_STRUCTURAL_NOT_ADMITTED" if passed else "FAIL_REGRESSION_NOT_ADMITTED", "revision": REVISION,
         "scope": "q10/Q_SCALE30/max-abs coupling; only long residue end changes 1760 to 2048; original guard unchanged",
         "metrics_contract": "Frame0 same-position, no shift, gain fit, clipping or normalization; bands are summed unwindowed <=1s block DFT energies, rows below/above 20625Hz; channels L/R",
         "cases": rows, "binary_sha256": {name: digest(binary) for name, binary in binaries.items()},
         "strict_reader_sha256": digest(root / "strict-reader"),
         "not_run": ["human listening", "native other platforms", "seek", "production admission", "finite-only fullband encoding"]})


if __name__ == "__main__":
    main()
