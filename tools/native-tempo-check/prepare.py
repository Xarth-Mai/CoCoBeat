#!/usr/bin/env python3
"""Lightweight stdlib QA snapshot, without PCM reads or native execution"""
import hashlib
import json
from pathlib import Path
import shutil
import sys

ROOT = Path(__file__).resolve().parents[2]
BTT = ROOT / "target/native-tempo-plan-20261007/btt-source"
MODEL = ROOT / "target/mir-model-20261007"
COMMIT = "c039090f1af771092d95c3ffc402e557940f7384"
MANIFEST_SHA = "570e4591dda2e15c586ff03bcf601a6f24e4306ddea3f75cca16a2df52a1f7c8"
MATRIX_SHA = {
    "source": "27f43448b0608c9c0f03cabfe3188c428b8f47003f678adc7a856effd22967bb",
    "canonical": "ddcb0fbc64e094c8a9accd1ed3b76ead95ae8ec1a1953225b49b9d5dbcdec250",
}
MEDIA_DRIVER_SHA = "199953f3115af196bc2262c72a4fc1928341192608cbe726838e569f7c92449a"
RECEIPTS_SHA = "46d463e2b7ee24d59ad17072467717c80e47a67f6e8f913d13a51a7b0da47fc8"
CASES = ["fixed_120", "fractional_123_45", "accelerando_90_150", "meter_3_4", "meter_6_8",
         "pickup_two_beats", "swing_2_to_1", "silence", "antiphase", "right_only"]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def require(condition, message):
    if not condition:
        raise ValueError(message)


def prepare(output):
    raw = (BTT / "manifest.json").read_bytes()
    require(sha(raw) == MANIFEST_SHA, "BTT manifest changed")
    manifest = json.loads(raw)
    require(manifest["commit"] == COMMIT, "BTT commit differs")
    for item in manifest["files"]:
        data = (BTT / item["path"]).read_bytes()
        blob = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
        require(sha(data) == item["sha256"] and blob == item["git_blob"]
                and len(data) == item["bytes"], "BTT source changed: " + item["path"])
    matrices = {}
    for kind, expected in MATRIX_SHA.items():
        path = MODEL / (kind + "-matrix") / "matrix.json"
        raw = path.read_bytes()
        require(sha(raw) == expected, kind + " matrix changed")
        matrices[kind] = json.loads(raw)
        require([c["name"] for c in matrices[kind]["cases"]] == CASES, "matrix case set differs")
        for case in matrices[kind]["cases"]:
            pcm = path.parent / case["pcm"]
            require(pcm.name == case["pcm"] and not pcm.is_symlink()
                    and pcm.is_file() and pcm.stat().st_size == case["frames"] * 8,
                    "missing or wrong-sized old PCM: " + str(pcm))
            require(case["sample_rate"] == 48000 and case["channels"] == 2
                    and case["frames"] == 1536000, "old PCM shape differs")
    receipt_path = MODEL / "canonical-matrix/readback-receipts.json"
    require(sha(receipt_path.read_bytes()) == RECEIPTS_SHA, "historical receipts changed")
    receipts = json.loads(receipt_path.read_bytes())
    require(receipts["driver_sha256"] == MEDIA_DRIVER_SHA
            and receipts["source_matrix_sha256"] == MATRIX_SHA["source"], "readback identity differs")
    require(len(receipts["cases"]) == 10, "incomplete readback receipts")
    for source, canonical, receipt in zip(matrices["source"]["cases"], matrices["canonical"]["cases"], receipts["cases"]):
        require({k: v for k, v in source.items() if k not in ("pcm", "pcm_sha256", "origin")}
                == {k: v for k, v in canonical.items() if k not in ("pcm", "pcm_sha256", "origin")},
                "source/canonical labels differ")
        require(receipt["name"] == source["name"] and receipt["source_pcm_sha256"] == source["pcm_sha256"]
                and receipt["pcm_sha256"] == canonical["pcm_sha256"], "readback receipt differs")
        require([call["command"][1] for call in receipt["calls"]] == ["encode", "readback"]
                and all(call["exit_code"] == 0 for call in receipt["calls"]), "readback was not complete")
    output.parent.mkdir(parents=True, exist_ok=True)
    output.mkdir()
    shutil.copytree(BTT, output / "btt")
    frozen = output / "frozen"
    frozen.mkdir()
    for name in ("driver.c", "build.sh", "prepare.py", "check.py", "README.md"):
        shutil.copyfile(Path(__file__).parent / name, frozen / name)
    for kind in MATRIX_SHA:
        shutil.copyfile(MODEL / (kind + "-matrix") / "matrix.json", frozen / (kind + "-matrix.json"))
    shutil.copyfile(receipt_path, frozen / "readback-receipts.json")
    # This current recipe reference is not represented as the historical full generator file
    recipe_source = ROOT / "tools/beat-model-probe/probe.py"
    shutil.copyfile(recipe_source, frozen / "beat-model-recipe.py")
    recipe = {
        "status": "PREPARED_NOT_RUN", "btt_commit": COMMIT,
        "pcm_roots": {kind: str(MODEL / (kind + "-matrix")) for kind in MATRIX_SHA},
        "matrices_sha256": MATRIX_SHA, "record_count": 40,
        "canonical_driver_sha256": MEDIA_DRIVER_SHA,
        "current_recipe_source_sha256": sha(recipe_source.read_bytes()),
        "historical_generator_sha256": matrices["source"]["generator_sha256"],
        "reference": {
            "source": "unchanged beat_seconds and reference_beat_unit in frozen metadata",
            "interval": "[round(beat[i]*48000), round(beat[i+1]*48000))",
            "bpm": "60*48000/(end-start), interval mean, not an exact instantaneous tempo",
            "stable_cases": [name for name in CASES if name not in ("accelerando_90_150", "silence")],
            "variable_cases": ["accelerando_90_150"],
            "silent_channels": "original silent_channels has no scalar reference",
            "outside_intervals": "unscored, not inferred silence or missing beats",
            "six_eight": "original dotted_quarter unit, no harmonic correction",
        },
        "quality_status": "UNSCORED_NO_ADMISSION_THRESHOLD", "confidence": None,
        "production_admission": False,
        "pcm_hash_verification": "NOT_RUN during light preparation; required before and after execution",
        "execution": {"pilot_deadline_seconds": 60, "matrix_deadline_seconds": 300,
                      "per_process_deadline_seconds": 30},
    }
    (output / "recipe.json").write_text(json.dumps(recipe, ensure_ascii=False, indent=2) + "\n")
    files = sorted(path for path in output.rglob("*") if path.is_file())
    (output / "source-files.sha256").write_text("".join(
        f"{sha(path.read_bytes())}  {path.relative_to(output)}\n" for path in files))
    print(output)


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: prepare.py NEW_PREPARED_DIRECTORY")
    prepare(Path(sys.argv[1]).resolve())
