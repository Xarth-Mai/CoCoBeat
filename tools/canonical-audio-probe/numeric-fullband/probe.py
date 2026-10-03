#!/usr/bin/env python3
"""Four frozen sign-kernel sources, outward resampler bounds and scratch codec diagnostics"""
import argparse
import difflib
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

import numpy as np

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
OLD = REPO / "target/resample-l1-domain-20261003"
FULLBAND = REPO / "target/fullband-regression-20261003"
RATES = (8000, 44100, 96000, 192000)
STAGES = ["mdct", "psy_band_energy", "masking_target", "floor_curve", "uncoupled_residue",
          "coupled_residue", "vq_work", "vq_cost", "mean_log_power", "geometric_power",
          "spread_power", "rd_cost", "encoder_pcm", "flatness_ratio"]
spec = importlib.util.spec_from_file_location("fullband_metrics", HERE.parent / "fullband-regression/probe.py")
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)
digest, save, load, run = helper.digest, helper.save, helper.load, helper.run


def replace_once(text, before, after):
    assert text.count(before) == 1, f"source changed: {before}"
    return text.replace(before, after)


def freeze_policy(root):
    assert digest(OLD / "FROZEN.json") == "3a89feec13c3062d1615172f3e5579b65d07c0d4e7f3c6d8e34feb23df4c7c2c"
    assert digest(FULLBAND / "FROZEN.json") == "09c3f20472f3ef52df8abc60b827010a73d8dd30328a0103408293db1e7fca18"
    index = load(OLD / "FROZEN.json")["sha256"]
    inputs = []
    for rate in RATES:
        names = [f"{rate}/source.wav", *[f"{rate}/candidate/{name}" for name in
                 ("resampled.f32le", "canonical.ogg", "decoded.f32le", "report.json")],
                 f"{rate}/ffmpeg.f32le", f"{rate}/result.json"]
        hashes = {name: digest(OLD / name) for name in names}
        assert all(index[name] == value for name, value in hashes.items())
        inputs.append({"rate": rate, "source_frames": rate, "output_frames": 48000,
                       "decoded_source_peak": 32700 / 32768, "sha256": hashes})
    if (root / "POLICY.json").exists():
        assert load(root / "POLICY.json")["inputs"] == inputs
    else:
        save(root / "POLICY.json", {"inputs": inputs, "scope": __doc__, "source_commit": "e1b3a26",
             "parameters": {"vorbis_q": 10, "Q_SCALE": 30, "other_VORBIS": "removed", "input_guard": "finite-only diagnostic"},
             "execution": {"max_source_encodes": 4, "seconds_per_encode_decode": 30, "address_space_bytes": 2147483648},
             "assumptions": ["Actual decoded source peak bounded by1", "Primitive IEEE round-to-nearest with no fast-math",
                             "Certificate binds actual generated coefficient table on this host", "No quality or production admission"]})
    save(root / "policy-identity.json", {"sha256": digest(root / "POLICY.json")})


def scratch_sources(root):
    index = load(FULLBAND / "FROZEN.json")["sha256"]
    for name, expected in index.items():
        if name.startswith(("snapshot/", "fullband/")) or name == "strict-reader":
            assert digest(FULLBAND / name) == expected
    shutil.copytree(FULLBAND / "snapshot", root / "snapshot")
    candidate = root / "candidate"
    shutil.copytree(FULLBAND / "fullband", candidate)
    shutil.copy2(FULLBAND / "strict-reader", root / "strict-reader")
    manifest = candidate / "Cargo.toml"
    manifest.write_text(replace_once(manifest.read_text(), str(FULLBAND / "snapshot"), str(root / "snapshot")))
    changed = {}

    def patch(path, changes):
        file = candidate / path
        original = file.read_text()
        text = original
        for before, after in changes:
            text = replace_once(text, before, after)
        file.write_text(text)
        changed[path] = {"before_sha256": hashlib.sha256(original.encode()).hexdigest(), "after_sha256": digest(file)}
        return "".join(difflib.unified_diff(original.splitlines(True), text.splitlines(True), fromfile="fullband/" + path, tofile="numeric/" + path))

    diffs = patch("src/main.rs", [
        ("if args.len() != 2 {", 'if args.len() == 1 && args[0] == "--numeric-budget" {\n        println!("{}", rusty_vorbis::numeric::budget());\n        return;\n    }\n    if args.len() != 2 {'),
        ("!(-1.0..=1.0).contains(value)", "!value.is_finite()"),
        ("PASS_SOFTWARE_CANDIDATE", "PASS_NUMERIC_FULLBAND_DIAGNOSTIC_ONLY"),
        ("rusty_vorbis-0.1.1-max-abs-q10", "rusty_vorbis-0.1.1-max-abs-q10-fullband-finite-diagnostic"),
        ("// The upstream API documents [-1, 1]; media legitimately accepts wider finite PCM", "// Diagnostic input is independently bounded by the frozen High certificate"),
        ("candidate unsupported input domain: resampled PCM must be finite and in [-1, 1] per rusty_vorbis::push_pcm_f32; no normalization or clipping applied", "numeric diagnostic rejects non-finite PCM; source and encoder budgets are independently checked"),
        ("let interleaved: Vec<_> = frames.iter().flatten().copied().collect();", "let interleaved: Vec<_> = frames.iter().flatten().copied().collect();\n        rusty_vorbis::numeric::observe(12, &interleaved);"),
        ("let readback = readback(output, input.frames)?;", "rusty_vorbis::numeric::dump();\n    let readback = readback(output, input.frames)?;"),
    ])
    prefix = "vendor/rusty_vorbis/src/"
    diffs += patch(prefix + "lib.rs", [("mod mdct;", "mod mdct;\npub mod numeric;")])
    shutil.copyfile(HERE / "codec_numeric.rs", candidate / prefix / "numeric.rs")
    mdct = candidate / prefix / "mdct.rs"
    mdct.write_text(mdct.read_text() + '''
// Scratch-only inspection of the actual constants used by the fixed long transform
pub(crate) fn numeric_constants_valid() -> bool {
    let t = mdct_twiddles(2048).unwrap();
    [&t.pre_c, &t.pre_s, &t.post_c, &t.post_s, &t.fft_c, &t.fft_s]
        .into_iter().flatten().all(|v| v.is_finite() && v.abs() <= 1.0)
        && vorbis_window(2048).iter().all(|v| v.is_finite() && v.abs() <= 1.0)
}
''')
    diffs += patch(prefix + "frame.rs", [
        ("i += dim;\n    }\n    Ok(bits)", "i += dim;\n    }\n    crate::numeric::observe(6, seg);\n    Ok(bits)"),
        ("Ok((work.iter().map(|x| x * x).sum(), bits))", "let distortion: f32 = work.iter().map(|x| x * x).sum();\n    crate::numeric::value(11, distortion);\n    Ok((distortion, bits))"),
        ("let cost = dist + lambda * bits as f32;", "let cost = dist + lambda * bits as f32;\n            crate::numeric::value(11, cost);"),
        ("let target = psy::masking_curve(&spectra[c], sample_rate, quality);", "crate::numeric::observe(0, &spectra[c]);\n        let target = psy::masking_curve(&spectra[c], sample_rate, quality);\n        crate::numeric::observe(2, &target);"),
        ("let curve = floor::fit_and_encode_floor(&mut bw, &target, fl, &setup.codebooks, m)?;", "let curve = floor::fit_and_encode_floor(&mut bw, &target, fl, &setup.codebooks, m)?;\n        crate::numeric::observe(3, &curve);"),
        ("// Forward channel coupling (in order; decode inverse-couples in reverse).", "for channel in &residue { crate::numeric::observe(4, channel); }\n    // Forward channel coupling (in order; decode inverse-couples in reverse)."),
        ("// Point stereo: collapse", "for channel in &residue { crate::numeric::observe(5, channel); }\n    // Point stereo: collapse"),
    ])
    diffs += patch(prefix + "psy.rs", [
        ("energy[b] = sum;", "energy[b] = sum;\n        crate::numeric::value(1, sum);\n        crate::numeric::value(8, logsum / cnt);"),
        ("let geo = (logsum / cnt).exp();", "let geo = (logsum / cnt).exp();\n        crate::numeric::value(9, geo);"),
        ("let sfm_db = 10.0 * (geo / arith).clamp", "let flatness = geo / arith;\n        crate::numeric::value(13, flatness);\n        let sfm_db = 10.0 * flatness.clamp"),
        ("thr[i] = spread *", "crate::numeric::value(10, spread);\n        thr[i] = spread *"),
    ])
    diffs += patch(prefix + "setup.rs", [
        ("let d = (v - lvl) * (v - lvl);", "let d = (v - lvl) * (v - lvl);\n                    crate::numeric::value(7, d);"),
        ("let best_cost = cost.iter().copied().fold(f32::INFINITY, f32::min);", "crate::numeric::observe(7, cost);\n        let best_cost = cost.iter().copied().fold(f32::INFINITY, f32::min);"),
    ])
    (root / "instrumentation.patch").write_text(diffs)
    save(root / "patched-source-identity.json", changed)
    return candidate, kernel_sources(root)


def kernel_sources(root):
    kernel = root / "kernel"
    (kernel / "src").mkdir(parents=True, exist_ok=True)
    for name in ("Cargo.toml", "Cargo.lock"):
        shutil.copyfile(OLD / "kernel" / name, kernel / name)
    old_module = (OLD / "kernel/src/resample_kernel.rs").read_text()
    upstream = old_module.split("\n// Diagnostic appended outside the frozen upstream source;")[0]
    assert hashlib.sha256(upstream.encode()).hexdigest() == "fe796ee4ebb67ce290adda9c11f98425ba3080c53e480049376980e9dfacfd48"
    (kernel / "src/resample_kernel.rs").write_text(upstream + (HERE / "kernel_bound.rs").read_text())
    (kernel / "src/main.rs").write_text('''pub use oximedia_audio::{AudioBuffer, AudioError, AudioFrame, AudioResult, ChannelLayout};
#[allow(dead_code)] mod resample_kernel;
fn main() {
    let output = std::env::args().nth(1).expect("output directory");
    for rate in [1, 8000, 44100, 96000, 192000] {
        println!("{}", resample_kernel::numeric_certificate(rate, &std::path::Path::new(&output).join(format!("table-{rate}.f64le"))));
    }
}
''')
    return kernel


def build(root, project, label):
    command = ["cargo", "build", "--offline", "--locked", "--release", "-j2", "--manifest-path", project / "Cargo.toml", "--target-dir", root / "build"]
    with (root / f"{label}-build.log").open("w") as log:
        result = subprocess.run(list(map(str, command)), stdout=log, stderr=subprocess.STDOUT, timeout=300)
    save(root / f"{label}-build.json", {"command": list(map(str, command)), "exit_code": result.returncode})
    result.check_returncode()


def case(root, rate, binary, certificate):
    output = root / str(rate)
    output.mkdir()
    source = OLD / str(rate) / "source.wav"
    prior = OLD / str(rate) / "candidate"
    row = {"rate": rate, "source_sha256": digest(source), "status": "FAIL"}
    try:
        row["encode"] = run([binary, source, output / "candidate"], output / "encode", seconds=30, resources=True)
        assert row["encode"]["exit_code"] == 0, (output / "encode.stderr").read_text()
        current = output / "candidate"
        assert digest(current / "resampled.f32le") == digest(prior / "resampled.f32le")
        pcm = np.fromfile(current / "resampled.f32le", dtype="<f4")
        row["resampled_peak"] = float(np.max(abs(pcm)))
        assert row["resampled_peak"] <= certificate["output_f32_upper_for_fixture_peak"]
        row["numeric_observations"] = json.loads(next(line[8:] for line in (output / "encode.stderr").read_text().splitlines() if line.startswith("NUMERIC ")))
        assert len(row["numeric_observations"]["calls"]) == len(STAGES)
        assert all(count > 0 for count in row["numeric_observations"]["calls"])
        row["strict"] = run([root / "strict-reader", current / "canonical.ogg", 48000, output / "strict.f32le"], output / "strict", seconds=30, resources=True)
        row["ffmpeg"] = run(["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "warning", "-xerror", "-err_detect", "explode", "-c:a", "vorbis", "-i", current / "canonical.ogg", "-map", "0:a:0", "-vn", "-c:a", "pcm_f32le", "-f", "f32le", "-n", output / "ffmpeg.f32le"], output / "ffmpeg", seconds=30, resources=True)
        assert row["strict"]["exit_code"] == row["ffmpeg"]["exit_code"] == 0
        a, b = [np.fromfile(output / name, dtype="<f4") for name in ("strict.f32le", "ffmpeg.f32le")]
        assert len(a) == len(b) == len(pcm) == 96000
        assert np.isfinite(a).all() and np.isfinite(b).all()
        row["dual_decoder_max_abs_difference"] = float(np.max(abs(a - b)))
        assert row["dual_decoder_max_abs_difference"] <= 1e-6
        assert digest(output / "strict.f32le") == digest(current / "decoded.f32le")
        row["ogg"] = helper.ogg(current / "canonical.ogg")
        assert row["ogg"]["eos_granules"] == [48000]
        row["baseline_metrics"] = helper.measure(prior / "resampled.f32le", prior / "decoded.f32le")
        row["combined_metrics"] = helper.measure(current / "resampled.f32le", output / "strict.f32le")
        row["snr_delta_db"] = [b - a for a, b in zip(row["baseline_metrics"]["whole"]["snr_db"], row["combined_metrics"]["whole"]["snr_db"])]
        row["status"] = "PASS_FIXED_COMBINATION_WITH_QUALITY_OBSERVATIONS_NOT_ADMITTED"
    except Exception as error:
        row["error"] = repr(error)
    save(output / "result.json", row)
    print(json.dumps({"rate": rate, "status": row["status"], "snr_delta_db": row.get("snr_delta_db")}), flush=True)
    return row


def run_probe(root, candidate, kernel):
    build(root, candidate, "candidate")
    build(root, kernel, "kernel")
    binary = root / "build/release/cocobeat-rusty-candidate"
    shutil.copy2(binary, root / "numeric-encoder")
    result = run([root / "build/release/cocobeat-kernel-domain-diagnostic", root], root / "kernel-bound", seconds=30, resources=True)
    assert result["exit_code"] == 0
    certificates = [json.loads(line) for line in (root / "kernel-bound.stdout").read_text().splitlines()]
    for item in certificates:
        item["table_sha256"] = digest(root / f'table-{item["source_rate"]}.f64le')
    assert len({item["table_sha256"] for item in certificates if item["source_rate"] < 48000}) == 1
    save(root / "kernel-certificates.json", {"command": result, "certificates": certificates})
    budget = run([binary, "--numeric-budget"], root / "codec-budget", seconds=30, resources=True)
    assert budget["exit_code"] == 0, (root / "codec-budget.stderr").read_text()
    save(root / "codec-budget.json", {"command": budget, "budget": load(root / "codec-budget.stdout"), "stage_names": STAGES})
    freeze_execution(root)
    rows = [case(root, rate, binary, next(item for item in certificates if item["source_rate"] == rate)) for rate in RATES]
    save(root / "results.json", {"status": "PASS_LIMITED_NOT_ADMITTED" if all(row["status"].startswith("PASS_") for row in rows) else "FAIL_LIMITED_NOT_ADMITTED", "policy_sha256": digest(root / "POLICY.json"), "cases": rows, "binary_sha256": digest(binary), "strict_reader_sha256": digest(root / "strict-reader"), "stage_names": STAGES, "not_run": ["other sources", "other downsampling rates", "native other platforms", "human listening", "seek", "production admission"]})


def freeze_execution(root):
    files = [path for directory in ("candidate", "snapshot", "kernel")
             for path in (root / directory).rglob("*") if path.is_file()]
    files += [root / name for name in ("numeric-encoder", "strict-reader", "POLICY.json")]
    save(root / "execution-identity.json", {
        "sha256": {str(path.relative_to(root)): digest(path) for path in sorted(files)},
        "tools": {str(path.relative_to(REPO)): digest(path) for path in
                  [HERE / "probe.py", HERE / "kernel_bound.rs", HERE / "codec_numeric.rs",
                   HERE.parent / "fullband-regression/probe.py", HERE.parent / "oxideav/probe.py"]}})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    root = args.output.resolve()
    root.mkdir(exist_ok=True)
    assert {p.name for p in root.iterdir()} <= {"POLICY.json", "independent-source-review.md"}, "output already used"
    freeze_policy(root)
    for file in HERE.iterdir():
        if file.is_file():
            shutil.copyfile(file, root / file.name)
    removed = {key: os.environ.pop(key) for key in list(os.environ) if key.startswith("VORBIS_")}
    os.environ["VORBIS_Q_SCALE"] = "30"
    save(root / "environment.json", {"removed_VORBIS": removed, "fixed_VORBIS": {"VORBIS_Q_SCALE": "30"}, "rustc": subprocess.check_output(["rustc", "-Vv"], text=True), "ffmpeg": subprocess.check_output(["ffmpeg", "-version"], text=True).splitlines()[0], "helper_sha256": digest(HERE.parent / "fullband-regression/probe.py")})
    candidate, kernel = scratch_sources(root)
    run_probe(root, candidate, kernel)


if __name__ == "__main__":
    main()
