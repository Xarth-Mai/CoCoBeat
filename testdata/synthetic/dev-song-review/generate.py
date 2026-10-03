#!/usr/bin/env python3
"""Export source events for human review; no synthesis or onset detection

SPDX-License-Identifier: MPL-2.0
"""

import csv
import hashlib
import json
from collections import Counter
from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[3]
FRAMES = 3_072_000
INPUTS = {
    "crates/cocobeat-runtime/src/dev_song.rs": "3a14a6d09bf72be411cc952e93cae3284b38669ec3b1072ba0dd9f99ee89e09c",
    "target/dev-assets/cocobeat-64.wav": "3390dd080cb536fd4a598ea933618dd99b4bf697c0b2874ff5cb0e220feb09e8",
    "assets/dev/vertical_slice/anchors.csv": "843ad9d30c2d7eb1322248c91278d769af56422fbcf08422eee01b23b923953d",
    "assets/dev/vertical_slice/event_frames.csv": "0794ba13ab3bd82321299060b504c7cf0fa903f925a25f6c121ebc899c324ce9",
}


def generate():
    for name, expected in INPUTS.items():
        if hashlib.sha256((ROOT / name).read_bytes()).hexdigest() != expected:
            raise SystemExit(f"Source identity changed: {name}")
    with (ROOT / "assets/dev/vertical_slice/anchors.csv").open() as source:
        anchors = [
            {"id": int(row["anchor_id"]), "source_frame": int(row["song_frame"]),
             "origin": "existing_authored_anchor", "candidate_id": f"source-{row['song_frame']}"}
            for row in csv.DictReader(source)
        ]
    candidates = []
    for frame in range(0, FRAMES, 12_000):
        if 1_920_000 <= frame < 2_304_000:
            continue
        beat = frame // 24_000
        motif = 16 <= beat < 80 or 96 <= beat < 120
        voices = []
        if frame % 24_000 == 0:
            voices.append("kick")
        if frame % 48_000 == 0:
            voices.append("low")
        if motif and frame % 24_000 == 0:
            voices.append("motif")
        if not motif and frame % 96_000 == 0:
            voices.append("pad")
        if 16 <= beat < 120:
            voices.append("hat")
        if voices:
            candidates.append({
                "id": f"source-{frame}", "source_frame": frame, "voices": voices,
                "authored_anchor_ids": [a["id"] for a in anchors if a["source_frame"] == frame],
                "onset_review": {"status": "pending", "frame": None, "interval": None, "reviewer": None},
                "anchor_review": {"status": "pending", "playable": None, "reviewer": None},
            })
    with (ROOT / "assets/dev/vertical_slice/event_frames.csv").open() as source:
        boundaries = [
            {"event": row["event"], "source_frame": int(row["song_frame"]),
             "kind": "exclusive_end" if row["event"] == "exclusive_end" else "section_start"}
            for row in csv.DictReader(source)
            if row["event"].endswith("_start") or row["event"] == "exclusive_end"
        ]

    counts = Counter(voice for event in candidates for voice in event["voices"])
    frames = [event["source_frame"] for event in candidates]
    assert counts == {"kick": 112, "low": 56, "motif": 88, "pad": 6, "hat": 176}
    assert len(frames) == len(set(frames)) == 200 and sum(counts.values()) == 438
    assert frames == sorted(frames) and frames[0] == 0 and frames[-1] == 3_048_000
    assert all(0 <= frame < FRAMES and not 1_920_000 <= frame < 2_304_000 for frame in frames)
    assert [(a["id"], a["source_frame"]) for a in anchors] == list(enumerate(
        [1_248_000, 1_440_000, 1_632_000, 1_824_000, 2_400_000, 2_592_000, 2_784_000], 1))
    assert sum(len(event["authored_anchor_ids"]) for event in candidates) == 7
    assert [b["source_frame"] for b in boundaries] == [0, 384_000, 1_152_000, 1_920_000, 2_304_000, 2_880_000, FRAMES]
    assert boundaries[-1]["kind"] == "exclusive_end" and FRAMES not in frames

    return {
        "format": "cocobeat-dev-song-review-v1",
        "scope": "Source timeline only; human onset and Anchor playability review pending",
        "license": {"source_timeline": "CC0-1.0", "generator": "MPL-2.0",
                    "reference": "assets/dev/vertical_slice/README.md"},
        "generator": {"path": Path(__file__).resolve().relative_to(ROOT).as_posix(),
                      "sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()},
        "inputs": [{"path": name, "sha256": digest} for name, digest in INPUTS.items()],
        "audio": {"sample_rate": 48_000, "channels": 2, "sample_format": "PCM s16le",
                  "frames": FRAMES, "silence_interval": [1_920_000, 2_304_000],
                  "review_presentation": "stereo; hat has opposite polarity in left and right; mono averaging cancels it in this recipe"},
        "counts": {"voice_events": 438, "source_candidates": 200, "voices": dict(counts),
                   "structural_boundaries": len(boundaries), "authored_anchors": len(anchors)},
        "structural_boundaries": boundaries,
        "authored_anchors": anchors,
        "candidates": candidates,
    }


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("Usage: python3 testdata/synthetic/dev-song-review/generate.py <new-review.json>")
    result = generate()
    candidates = result.pop("candidates")
    # Keep each review row on one line, within one ordinary JSON document
    text = json.dumps(result, ensure_ascii=False, indent=2)[:-2]
    text += ',\n  "candidates": [\n'
    text += ",\n".join("    " + json.dumps(row, ensure_ascii=False) for row in candidates)
    text += "\n  ]\n}\n"
    with Path(sys.argv[1]).open("x", encoding="utf-8", newline="\n") as output:
        output.write(text)
    print("438 source voice events / 200 candidates; human review pending")
