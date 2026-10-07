#!/usr/bin/env python3
"""Research-only Beat This! export and channel-preserving numerical comparison"""

import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path
import platform
import subprocess
import time
import wave


def digest(path):
    with Path(path).open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def score(reference, predicted, tolerance=0.07):
    """Ordered one-to-one beat matching, independent of model scores"""
    i = j = 0
    errors = []
    while i < len(reference) and j < len(predicted):
        error = predicted[j] - reference[i]
        if abs(error) <= tolerance:
            errors.append(error)
            i += 1
            j += 1
        elif error < 0:
            j += 1
        else:
            i += 1
    count = len(errors)
    return {
        "tp": count,
        "fp": len(predicted) - count,
        "fn": len(reference) - count,
        "f1": 2 * count / (len(reference) + len(predicted))
        if reference or predicted else 1.0,
        "tolerance_seconds": tolerance,
        "max_matched_error_seconds": max(map(abs, errors), default=None),
    }


def matrix(destination):
    import numpy as np

    destination.mkdir(parents=True, exist_ok=True)
    rate, duration = 48_000, 32
    cases = []
    configs = [
        ("fixed_120", 120.0, 4, 0, False),
        ("fractional_123_45", 123.45, 4, 0, False),
        ("accelerando_90_150", 90.0, 4, 0, False),
        ("meter_3_4", 120.0, 3, 0, False),
        ("meter_6_8", 90.0, 2, 0, False),
        ("pickup_two_beats", 120.0, 4, 2, False),
        ("swing_2_to_1", 120.0, 4, 0, True),
        ("silence", 120.0, 4, 0, False),
        ("antiphase", 120.0, 4, 0, False),
        ("right_only", 120.0, 4, 0, False),
    ]
    for name, bpm, beats_per_bar, pickup, swing in configs:
        audio = np.zeros(rate * duration, dtype=np.float64)
        beats, downbeats = [], []
        rng = np.random.default_rng(271828)

        def hit(at, accent=False, subdivision=False):
            start = round(at * rate)
            count = min(round(0.10 * rate), len(audio) - start)
            if count <= 0:
                return
            t = np.arange(count) / rate
            tone = np.sin(2 * np.pi * (65 if accent else 110) * t)
            noise = rng.normal(0, 1, count)
            gain = 0.08 if subdivision else (0.6 if accent else 0.35)
            audio[start:start + count] += gain * (
                tone * np.exp(-t * 45) + 0.15 * noise * np.exp(-t * 180)
            )

        at, index = 1.0, 0
        while at < duration - 0.5 and name != "silence":
            current_bpm = 90 + 60 * (at - 1) / (duration - 1) if name.startswith("accelerando") else bpm
            interval = 60 / current_bpm
            accent = (index + pickup) % beats_per_bar == 0
            at = round(at * rate) / rate
            beats.append(at)
            if accent:
                downbeats.append(at)
            hit(at, accent)
            if swing:
                hit(at + interval * 2 / 3, subdivision=True)
            if name == "meter_6_8":
                for subdivision in (1, 2):
                    hit(at + interval * subdivision / 3, subdivision=True)
            at += interval
            index += 1
        stereo = np.column_stack((audio, -audio if name == "antiphase" else audio))
        if name == "right_only":
            stereo[:, 0] = 0
        assert np.max(np.abs(stereo)) < 1
        quantized = np.rint(stereo * 32767).astype("<i2")
        with wave.open(str(destination / f"{name}.wav"), "wb") as output:
            output.setparams((2, 2, rate, len(audio), "NONE", "not compressed"))
            output.writeframes(quantized.tobytes())
        pcm = (quantized.astype(np.float32) / 32768).astype("<f4")
        pcm_path = destination / f"{name}.f32le"
        pcm.tofile(pcm_path)
        if name == "antiphase":
            assert np.array_equal(pcm[:, 0], -pcm[:, 1])
        cases.append({
            "name": name, "pcm": pcm_path.name, "pcm_sha256": digest(pcm_path),
            "sample_rate": rate, "channels": 2, "frames": len(audio),
            "origin": "synthetic_source_not_canonical",
            "reference_beat_unit": "dotted_quarter" if name == "meter_6_8" else "quarter",
            "beat_seconds": beats, "downbeat_seconds": downbeats,
            "silent_channels": [0, 1] if name == "silence" else ([0] if name == "right_only" else []),
        })
    write_json(destination / "matrix.json", {
        "purpose": "independent synthetic software stress matrix, not real-music quality admission",
        "license": "CC0-1.0", "generator_sha256": digest(__file__), "cases": cases,
    })
    assert score([1, 2], [0.5, 1.02, 2.06, 3])["tp"] == 2
    assert score([], [1])["fp"] == 1
    assert score([], [])["f1"] == 1
    print(destination / "matrix.json")


def prepare(args):
    checkpoint = args.checkpoint.resolve(strict=True)
    if digest(checkpoint) != args.checkpoint_sha256:
        raise ValueError("checkpoint SHA-256 differs from frozen official download")
    import torch
    from beat_this.inference import Audio2Frames

    torch.set_num_threads(2)
    torch.set_num_interop_threads(1)
    torch.manual_seed(271828)
    reference = Audio2Frames(checkpoint_path=str(checkpoint), device="cpu", float16=False)
    return torch, reference


def canonicalize(args):
    import numpy as np

    source_manifest = args.matrix.read_bytes()
    manifest = json.loads(source_manifest)
    if not manifest["cases"]:
        raise ValueError("empty source matrix")
    driver = args.driver.resolve(strict=True)
    driver_hash = digest(driver)
    matrix_hash = hashlib.sha256(source_manifest).hexdigest()
    identities = {driver: driver_hash, args.matrix: matrix_hash}
    args.destination.mkdir(parents=True, exist_ok=False)
    receipts = []
    for index, case in enumerate(manifest["cases"]):
        calls = []
        try:
            frames = case["frames"]
            if case["sample_rate"] != 48_000 or case["channels"] != 2 or not 0 < frames <= 120 * 48_000:
                raise ValueError("expected bounded 48 kHz stereo source PCM")
            source_pcm = args.matrix.parent / case["pcm"]
            source_bytes = source_pcm.read_bytes()
            if len(source_bytes) != frames * 8 or hashlib.sha256(source_bytes).hexdigest() != case["pcm_sha256"]:
                raise ValueError("source PCM differs from source manifest")
            source = source_pcm.with_suffix(".wav")
            source_hash = digest(source)
            with wave.open(str(source), "rb") as wav:
                if (wav.getnchannels(), wav.getsampwidth(), wav.getframerate(), wav.getnframes(), wav.getcomptype()) != (2, 2, 48_000, frames, "NONE"):
                    raise ValueError("source WAV format differs from source manifest")
                samples = wav.readframes(frames + 1)
            quantized = (np.frombuffer(samples, dtype="<i2").astype(np.float32) / 32768).astype("<f4")
            if quantized.tobytes() != source_bytes:
                raise ValueError("source WAV samples differ from reference source PCM")
            identities.update({source: source_hash, source_pcm: case["pcm_sha256"]})
            ogg = args.destination / f"{index:02}.ogg"
            pcm = args.destination / f"{index:02}.f32le"
            commands = [
                ([str(driver), "encode", str(source), str(ogg)],
                 {"source_rate": 48_000, "source_frames": frames, "frames": frames}),
                ([str(driver), "readback", str(ogg), str(frames), str(pcm)],
                 {"frames": frames, "channels": 2, "sample_rate": 48_000}),
            ]
            for command, expected_metadata in commands:
                if any(digest(path) != identity for path, identity in identities.items()):
                    raise ValueError("source, manifest, driver or encoded OGG identity changed")
                result = subprocess.run(command, capture_output=True, text=True, timeout=120)
                calls.append({"command": command, "exit_code": result.returncode,
                              "stdout": result.stdout, "stderr": result.stderr})
                if result.returncode:
                    raise ValueError("canonical driver failed")
                if json.loads(result.stdout) != expected_metadata:
                    raise ValueError("canonical driver metadata differs from source contract")
                if any(digest(path) != identity for path, identity in identities.items()):
                    raise ValueError("source, manifest, driver or encoded OGG identity changed")
                if command[1] == "encode":
                    identities[ogg] = digest(ogg)
            output_bytes = pcm.read_bytes()
            if len(output_bytes) != frames * 8 or not np.isfinite(np.frombuffer(output_bytes, dtype="<f4")).all():
                raise ValueError("strict readback has invalid length or nonfinite samples")
        except (OSError, ValueError, wave.Error, subprocess.TimeoutExpired) as error:
            write_json(args.destination / "failed-call.json", {"case": case["name"], "error": str(error), "calls": calls})
            raise
        receipts.append({"name": case["name"], "source_sha256": source_hash,
                         "source_pcm_sha256": case["pcm_sha256"],
                         "ogg_sha256": identities[ogg], "pcm_sha256": digest(pcm), "calls": calls})
        case.update({"pcm": pcm.name, "pcm_sha256": digest(pcm), "origin": "canonical_strict_readback"})
        write_json(args.destination / "readback-receipts.json", {
            "driver_sha256": driver_hash, "source_matrix_sha256": matrix_hash,
            "cases": receipts,
        })
    if any(digest(path) != identity for path, identity in identities.items()):
        raise ValueError("canonicalization input identity changed before publication")
    write_json(args.destination / "matrix.json", manifest)


def export(args):
    import numpy as np
    import onnx
    torch, reference = prepare(args)

    class Outputs(torch.nn.Module):
        def __init__(self, model):
            super().__init__()
            self.model = model

        def forward(self, spect):
            output = self.model(spect)
            return output["beat"], output["downbeat"]

    wrapped = Outputs(reference.model).eval()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(
        wrapped, torch.zeros(1, 1500, 128), str(args.output),
        input_names=["spect"], output_names=["beat", "downbeat"],
        dynamic_axes={"spect": {1: "frames"}, "beat": {1: "frames"}, "downbeat": {1: "frames"}},
        opset_version=17, dynamo=False,
    )
    onnx.checker.check_model(str(args.output))
    session = ort_session(args.output)
    checks = []
    with torch.inference_mode():
        for frames in (128, 257, 1499, 1500):
            spect = torch.rand(1, frames, 128)
            expected = wrapped(spect)
            actual = session.run(None, {"spect": spect.numpy()})
            for name, left, right in zip(("beat", "downbeat"), expected, actual):
                left = left.numpy()
                checks.append({"frames": frames, "output": name,
                               "max_abs_error": float(np.max(np.abs(left - right))),
                               "allclose": bool(np.allclose(left, right, atol=1e-4, rtol=1e-4))})
    write_json(args.output.with_suffix(".export.json"), {
        "checkpoint_sha256": args.checkpoint_sha256,
        "onnx_sha256": digest(args.output), "checks": checks,
        "numeric_status": "PASS" if all(row["allclose"] for row in checks) else "FAIL",
        "production_admission": False,
    })
    if not all(row["allclose"] for row in checks):
        raise ValueError("ONNX export differs from official model")


def ort_session(path):
    import onnxruntime as ort

    ort.disable_telemetry_events()
    options = ort.SessionOptions()
    options.intra_op_num_threads = 2
    options.inter_op_num_threads = 1
    return ort.InferenceSession(str(path), sess_options=options, providers=["CPUExecutionProvider"])


def compare(args, postprocessors=None, postprocessing_metadata=None):
    import numpy as np
    from beat_this.inference import split_predict_aggregate
    from beat_this.model.postprocessor import Postprocessor

    torch, reference = prepare(args)
    session = ort_session(args.onnx)
    postprocessors = postprocessors or {"minimal": Postprocessor(type="minimal", fps=50)}
    manifest = json.loads(args.matrix.read_text())
    if not manifest["cases"]:
        raise ValueError("empty matrix cannot establish equivalence")
    rows = []
    started = time.perf_counter()

    def infer_ort(spect):
        beat, downbeat = session.run(None, {"spect": spect.numpy()})
        return {"beat": torch.from_numpy(beat), "downbeat": torch.from_numpy(downbeat)}

    for case in manifest["cases"]:
        if case["origin"] != "canonical_strict_readback" and not args.allow_source_smoke:
            raise ValueError("quality input must be final strict canonical readback")
        pcm_path = args.matrix.parent / case["pcm"]
        if digest(pcm_path) != case["pcm_sha256"]:
            raise ValueError(f"PCM identity changed: {pcm_path}")
        if case["sample_rate"] != 48_000 or case["channels"] != 2 or not 0 < case["frames"] <= 120 * 48_000:
            raise ValueError("expected bounded 48 kHz stereo canonical PCM")
        pcm = np.fromfile(pcm_path, dtype="<f4").reshape(-1, 2)
        if len(pcm) != case["frames"] or not np.isfinite(pcm).all():
            raise ValueError("invalid PCM length or samples")
        for channel in range(2):
            with torch.inference_mode():
                spect = reference.signal2spect(pcm[:, channel].copy(), 48_000)
                before = time.perf_counter()
                expected = reference.spect2frames(spect)
                official_seconds = time.perf_counter() - before
                before = time.perf_counter()
                actual_dict = split_predict_aggregate(spect, 1500, 6, "keep_first", infer_ort)
                actual = (actual_dict["beat"], actual_dict["downbeat"])
                ort_seconds = time.perf_counter() - before
                for configuration, postprocessor in postprocessors.items():
                    before = time.perf_counter()
                    official_positions = postprocessor(*expected)
                    ort_positions = postprocessor(*actual)
                    postprocess_seconds = time.perf_counter() - before
                    for name, left, right, left_positions, right_positions in zip(
                        ("beat", "downbeat"), expected, actual, official_positions, ort_positions
                    ):
                        positions = right_positions.tolist()
                        truth = [] if channel in case["silent_channels"] else case[f"{name}_seconds"]
                        rows.append({
                            "case": case["name"], "origin": case["origin"], "channel": channel,
                            "configuration": configuration,
                            "output": name, "max_abs_logit_error": float(torch.max(torch.abs(left - right))),
                            "logits_allclose": bool(torch.allclose(left, right, atol=1e-4, rtol=1e-4)),
                            "positions_identical": bool(np.array_equal(left_positions, right_positions)),
                            "official_positions_seconds": left_positions.tolist(),
                            "ort_positions_seconds": positions,
                            "out_of_range_positions_seconds": [position for position in positions
                                if not 0 <= position < case["frames"] / case["sample_rate"]],
                            "quality": score(truth, positions),
                            "official_seconds": official_seconds, "ort_seconds": ort_seconds,
                            "postprocess_pair_seconds": postprocess_seconds,
                        })
    write_json(args.output, {
        "checkpoint_sha256": args.checkpoint_sha256, "onnx_sha256": digest(args.onnx),
        "matrix_sha256": digest(args.matrix), "probe_sha256": digest(__file__),
        "platform": platform.platform(), "elapsed_seconds": time.perf_counter() - started,
        "packages": {p: importlib.metadata.version(p) for p in ("torch", "torchaudio", "onnx", "onnxruntime", "beat-this", "numpy", "soxr", "einops", "rotary-embedding-torch")},
        "numeric_status": "PASS" if all(row["logits_allclose"] and row["positions_identical"] for row in rows) else "FAIL",
        "confidence_calibration": None, "onset_accuracy": "NOT_EVALUATED",
        "postprocessing_metadata": postprocessing_metadata,
        "production_admission": False, "rows": rows,
    })
    if not all(row["logits_allclose"] and row["positions_identical"] for row in rows):
        raise ValueError("ORT logits or coordinates differ from official model; see saved report")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    generate = commands.add_parser("matrix")
    generate.add_argument("destination", type=Path)
    canonical = commands.add_parser("canonicalize")
    canonical.add_argument("--matrix", type=Path, required=True)
    canonical.add_argument("--driver", type=Path, required=True)
    canonical.add_argument("--destination", type=Path, required=True)
    for name in ("export", "compare"):
        command = commands.add_parser(name)
        command.add_argument("--checkpoint", type=Path, required=True)
        command.add_argument("--checkpoint-sha256", required=True)
        command.add_argument("--output", type=Path, required=True)
        if name == "compare":
            command.add_argument("--onnx", type=Path, required=True)
            command.add_argument("--matrix", type=Path, required=True)
            command.add_argument("--allow-source-smoke", action="store_true")
    args = parser.parse_args()
    for variable in ("OMP_NUM_THREADS", "MKL_NUM_THREADS", "OPENBLAS_NUM_THREADS", "RAYON_NUM_THREADS"):
        os.environ[variable] = "2"
    if args.command == "matrix":
        matrix(args.destination)
    else:
        {"export": export, "compare": compare, "canonicalize": canonicalize}[args.command](args)


if __name__ == "__main__":
    main()
