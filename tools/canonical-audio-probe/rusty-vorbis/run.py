#!/usr/bin/env python3
"""Reproduce short encoder/oracle checks; commands are bounded and never play audio"""
import argparse
import array
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import sys

def run(args, directory, stem, seconds=30):
    try:
        result = subprocess.run(args, capture_output=True, text=True, timeout=seconds)
        record = {"argv": [str(a) for a in args], "exit": result.returncode, "timeout_seconds": seconds}
        (directory / f"{stem}.stdout").write_text(result.stdout)
        (directory / f"{stem}.stderr").write_text(result.stderr)
    except subprocess.TimeoutExpired:
        record = {"argv": [str(a) for a in args], "exit": "TIMEOUT", "timeout_seconds": seconds}
    return record

def pcm_metrics(path):
    if not path.exists():
        return None
    data = array.array("f")
    data.frombytes(path.read_bytes())
    if sys.byteorder != "little":
        data.byteswap()
    channels = [data[0::2], data[1::2]]
    def metric(values):
        finite = [v for v in values if math.isfinite(v)]
        return {"peak": max(map(abs, finite), default=0), "rms": math.sqrt(sum(v*v for v in finite) / max(len(finite), 1)), "nonfinite": len(values)-len(finite)}
    return {"frames": len(data)//2, "trailing_bytes": path.stat().st_size % 8,
            "channel_metrics": [metric(c) for c in channels],
            "first_128": [metric(c[:128]) for c in channels], "last_128": [metric(c[-128:]) for c in channels],
            "first_8_frames": list(zip(*[c[:8] for c in channels])), "last_8_frames": list(zip(*[c[-8:] for c in channels]))}

def decoder_difference(case):
    paths = [case / f"{name}.f32le" for name in ["symphonia", "ffmpeg"]]
    if not all(path.exists() for path in paths):
        return None
    buffers = []
    for path in paths:
        values = array.array("f", path.read_bytes())
        if sys.byteorder != "little":
            values.byteswap()
        buffers.append(values)
    return {"same_sample_count": len(buffers[0]) == len(buffers[1]),
            "maximum_sample_difference": max((abs(a-b) for a,b in zip(*buffers)), default=0)}

def ogg_pages(path):
    data = path.read_bytes()
    offset = 0
    pages = []
    while offset < len(data):
        assert data[offset:offset+4] == b"OggS"
        flags = data[offset+5]
        granule = struct.unpack_from("<Q", data, offset+6)[0]
        segments = data[offset+26]
        size = 27 + segments + sum(data[offset+27:offset+27+segments])
        assert offset+size <= len(data)
        pages.append({"offset": offset, "flags": flags, "granule": granule, "bytes": size})
        offset += size
    return pages

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("output_dir", type=Path)
    parser.add_argument("bin_dir", type=Path)
    args = parser.parse_args()
    root, bin_dir = args.output_dir.resolve(), args.bin_dir.resolve()
    results = []
    for frames,q in [(0,5),(1,5),(1024,5),(1025,5),(48000,5),(48128,5),(48000,0),(48000,10)]:
        case = root / "cases" / f"{frames}-q{q}"
        case.mkdir(parents=True, exist_ok=True)
        commands = [run([bin_dir/"encode",case,str(frames),str(q)],case,"encode")]
        if commands[0]["exit"] != 0:
            results.append({"frames":frames,"q":q,"commands":commands,"status":"FAIL"})
            continue
        commands.append(run([bin_dir/"readback",case/"encoded.ogg",case/"symphonia.f32le"],case,"symphonia"))
        commands.append(run(["ffprobe","-v","error","-show_streams","-show_format","-of","json",case/"encoded.ogg"],case,"ffprobe"))
        commands.append(run(["ffmpeg","-nostdin","-hide_banner","-v","error","-y","-i",case/"encoded.ogg","-map","0:a:0","-c:a","pcm_f32le","-f","f32le",case/"ffmpeg.f32le"],case,"ffmpeg"))
        metrics = {name:pcm_metrics(case/f"{name}.f32le") for name in ["input","symphonia","ffmpeg"]}
        pages = ogg_pages(case/"encoded.ogg")
        (case/"pages.json").write_text(json.dumps(pages,indent=2)+"\n")
        packet_records = [json.loads(line) for line in (case/"packets.jsonl").read_text().splitlines()]
        assert [p["granule"] for p in pages] == [p["pts"] for p in packet_records], "Mux changed official packet granules"
        encoded = (case/"encoded.ogg").read_bytes()
        symphonia_format = json.loads((case/"symphonia.stdout").read_text()) if commands[1]["exit"] == 0 else None
        ffprobe = json.loads((case/"ffprobe.stdout").read_text()) if commands[2]["exit"] == 0 else None
        ffmpeg_format = ({"sample_rate":int(ffprobe["streams"][0]["sample_rate"]), "channels":ffprobe["streams"][0]["channels"]} if ffprobe and ffprobe.get("streams") else None)
        failures = []
        for name,index in [("symphonia",1),("ffmpeg",3)]:
            result = metrics[name]
            if commands[index]["exit"] != 0:
                failures.append(f"{name} decode exit {commands[index]['exit']}")
            elif result is None or result["frames"] != frames:
                failures.append(f"{name} frames {None if result is None else result['frames']} != {frames}")
            if result and any(ch["nonfinite"] for ch in result["channel_metrics"]):
                failures.append(f"{name} nonfinite samples")
        for name,metadata in [("symphonia",symphonia_format),("ffmpeg",ffmpeg_format)]:
            if metadata and (metadata["sample_rate"] != 48000 or metadata["channels"] != 2):
                failures.append(f"{name} format mismatch {metadata}")
        if pages[-1]["granule"] != frames:
            failures.append(f"EOS granule {pages[-1]['granule']} != {frames}")
        result = {"frames":frames,"q":q,"quality":json.loads((case/"encode.stdout").read_text())["quality"],
                  "status":"FAIL" if failures else "PASS","failures":failures,"commands":commands,"metrics":metrics,
                  "decoder_formats":{"symphonia":symphonia_format,"ffmpeg":ffmpeg_format},"decoder_agreement":decoder_difference(case),
                  "ogg_bytes":len(encoded),"sha256":hashlib.sha256(encoded).hexdigest(),
                  "audio_packets":len(packet_records)-3,"eos_granule":pages[-1]["granule"],"has_eos":bool(pages[-1]["flags"]&4)}
        results.append(result)
        (case/"result.json").write_text(json.dumps(result,indent=2)+"\n")
        print(json.dumps({key:result[key] for key in ["frames","q","status","failures","ogg_bytes","audio_packets","eos_granule"]}),flush=True)
    overall = {"status":"FAIL" if any(r["status"] != "PASS" for r in results) else "PASS", "cases":results,
               "long_64_seconds":{"status":"NOT RUN","reason":"Short-sample correctness gate failed"}}
    (root/"results.json").write_text(json.dumps(overall,indent=2)+"\n")

if __name__ == "__main__":
    main()
