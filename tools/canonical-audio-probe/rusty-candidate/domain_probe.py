#!/usr/bin/env python3
"""Compare the guarded candidate with a finite-only scratch copy; never admit or play"""
import argparse
import difflib
import hashlib
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess
import sys

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
sys.path.insert(0, str(HERE.parent / "oxideav"))
from probe import metrics, ogg, read_pcm, run


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def save(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def wav(path, rate, samples):
    data = b"".join(struct.pack("<ff", *frame) for frame in samples)
    path.write_bytes(b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt "
                    + struct.pack("<IHHIIHH", 16, 3, 2, rate, rate * 8, 8, 32)
                    + b"data" + struct.pack("<I", len(data)) + data)


def diagnostic_binary(output):
    scratch = output / "finite-only"
    (scratch / "src").mkdir(parents=True)
    source = (HERE / "src/main.rs").read_text()
    replacements = {
        "!(-1.0..=1.0).contains(value)": "!value.is_finite()",
        "PASS_SOFTWARE_CANDIDATE": "PASS_DOMAIN_DIAGNOSTIC_ONLY",
        "rusty_vorbis-0.1.1-max-abs-q10": "rusty_vorbis-0.1.1-max-abs-q10-finite-domain-diagnostic",
        "candidate unsupported input domain: resampled PCM must be finite and in [-1, 1] per rusty_vorbis::push_pcm_f32; no normalization or clipping applied": "finite-domain diagnostic rejects non-finite PCM; no normalization or clipping applied",
        "// The upstream API documents [-1, 1]; media legitimately accepts wider finite PCM": "// Diagnostic only: deliberately exceed the upstream documented input domain",
    }
    patched = source
    for before, after in replacements.items():
        assert patched.count(before) == 1, f"candidate source changed: {before}"
        patched = patched.replace(before, after)
    (scratch / "src/main.rs").write_text(patched)
    (output / "finite-only.patch").write_text("".join(difflib.unified_diff(
        source.splitlines(True), patched.splitlines(True), fromfile="candidate/main.rs", tofile="diagnostic/main.rs")))
    manifest = (HERE / "Cargo.toml").read_text().replace(
        'path = "../../../crates/cocobeat-media"', f'path = "{REPO / "crates/cocobeat-media"}"').replace(
        'path = "vendor/rusty_vorbis"', f'path = "{HERE / "vendor/rusty_vorbis"}"')
    (scratch / "Cargo.toml").write_text(manifest)
    shutil.copyfile(HERE / "Cargo.lock", scratch / "Cargo.lock")
    command = ["cargo", "build", "--offline", "--locked", "--release", "-j1", "--manifest-path", str(scratch / "Cargo.toml"),
               "--target-dir", str(scratch / "target")]
    with (output / "build.log").open("w") as log:
        built = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=300)
    save(output / "build.json", {"command": command, "exit_code": built.returncode})
    built.check_returncode()
    return scratch / "target/release/cocobeat-rusty-candidate"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate_binary", type=Path)
    parser.add_argument("output_dir", type=Path)
    args = parser.parse_args()
    root = args.output_dir.resolve()
    root.mkdir()
    source_files = [Path(__file__), HERE.parent / "oxideav/probe.py", REPO / "Cargo.toml",
                    HERE / "src/main.rs", HERE / "Cargo.toml", HERE / "Cargo.lock", HERE / "upstream.json"]
    for package in [REPO / "crates/cocobeat-media", REPO / "crates/cocobeat-schema"]:
        source_files.extend([package / "Cargo.toml", *sorted((package / "src").rglob("*.rs"))])
    source_files.extend(path for path in sorted((HERE / "vendor/rusty_vorbis").rglob("*")) if path.is_file())
    source_hashes = {str(path.relative_to(REPO)): digest(path) for path in source_files}
    shutil.copyfile(__file__, root / "domain_probe.py")
    binary = diagnostic_binary(root)
    rows = []
    for name, rate, peak in [("step-44100", 44100, 32700 / 32768),
                             ("step-96000", 96000, 32700 / 32768),
                             ("sine-48000", 48000, 1.16)]:
        case = root / name
        case.mkdir()
        if name.startswith("step"):
            samples = [[-peak if i < rate // 20 else peak] * 2 for i in range(rate // 10)]
        else:
            samples = [[peak * math.sin(math.tau * frequency * i / rate) for frequency in (440, 1000)] for i in range(rate)]
        source = case / "source.wav"
        wav(source, rate, samples)
        guarded = run([args.candidate_binary.resolve(), source, case / "guarded"], case / "guarded", seconds=30, resources=True)
        guarded_report = json.loads((case / "guarded/report.json").read_text())
        assert guarded["exit_code"] == 1 and "unsupported input domain" in guarded_report["result"]["error"]
        assert not (case / "guarded/canonical.ogg").exists()
        encoded = run([binary, source, case / "diagnostic"], case / "encode", seconds=30, resources=True)
        assert encoded["exit_code"] == 0, (case / "encode.stderr").read_text()
        output = case / "diagnostic"
        report = json.loads((output / "report.json").read_text())
        assert report["result"]["status"] == "PASS_DOMAIN_DIAGNOSTIC_ONLY"
        decoded = run(["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "warning", "-xerror", "-err_detect", "explode",
                       "-c:a", "vorbis", "-i", output / "canonical.ogg", "-map", "0:a:0", "-vn",
                       "-c:a", "pcm_f32le", "-f", "f32le", "-n", case / "ffmpeg.f32le"], case / "ffmpeg", seconds=30, resources=True)
        assert decoded["exit_code"] == 0, (case / "ffmpeg.stderr").read_text()
        original = read_pcm(output / "resampled.f32le")
        a, b = read_pcm(output / "decoded.f32le"), read_pcm(case / "ffmpeg.f32le")
        container = ogg(output / "canonical.ogg")
        frames = len(original[0])
        assert frames == (len(samples) * 48000 + rate - 1) // rate
        assert container["eos_granules"] == [frames]
        assert container["channels"] == 2 and container["sample_rate"] == 48000
        assert all(len(channel) == frames and all(map(math.isfinite, channel)) for channel in a + b)
        error = max(abs(x - y) for left, right in zip(a, b) for x, y in zip(left, right))
        assert error <= 1e-6
        row = {"case": name, "status": "PASS_LIMITED_DOMAIN_DIAGNOSTIC", "source_rate": rate,
               "source_peak": max(abs(v) for frame in samples for v in frame), "source_sha256": digest(source),
               "guarded": guarded, "diagnostic": encoded, "ffmpeg": decoded,
               "report": report, "dual_decoder_max_abs_error": error,
               "input": metrics(output / "resampled.f32le", original),
               "symphonia": metrics(output / "decoded.f32le", original), "ffmpeg_pcm": metrics(case / "ffmpeg.f32le", original)}
        row["boundary_observations"] = {}
        for label, lo, hi in [("head", 0, 512), ("tail", frames - 512, frames)]:
            row["boundary_observations"][label] = []
            for reference, actual in zip(original, a):
                source_window, window = reference[lo:hi], actual[lo:hi]
                signal, energy = sum(v * v for v in source_window), sum(v * v for v in window)
                row["boundary_observations"][label].append({
                    "energy_ratio_db": 10 * math.log10(energy / signal),
                    "same_position_snr_db": 10 * math.log10(signal / sum((x - y) ** 2 for x, y in zip(source_window, window))),
                    "input_peak": max(map(abs, source_window)), "decoded_peak": max(map(abs, window))})
        row["sha256"] = {str(path.relative_to(case)): digest(path) for path in case.rglob("*") if path.is_file()}
        save(case / "result.json", row)
        rows.append(row)
        print(json.dumps({"case": name, "source_peak": row["source_peak"], "input_peak": report["result"]["evidence"]["resampled"]["peak"],
                          "snr_db": report["result"]["evidence"]["readback"]["snr_db"], "decoder_error": error}), flush=True)
    assert source_hashes == {str(path.relative_to(REPO)): digest(path) for path in source_files}, "sources changed during diagnostic"
    save(root / "result.json", {
        "status": "PASS_LIMITED_DOMAIN_DIAGNOSTIC_NOT_ADMITTED", "cases": rows,
        "scope": "Linux 3 synthetic cases only; source and PCM gain unchanged; no listening, music, arbitrary finite-domain or production claim",
        "source_sha256": source_hashes,
        "candidate_binary_sha256": digest(args.candidate_binary), "diagnostic_binary_sha256": digest(binary),
        "diagnostic_source_sha256": digest(root / "finite-only/src/main.rs"), "patch_sha256": digest(root / "finite-only.patch")})


if __name__ == "__main__":
    main()
