"""Read the OxiAudio outputs with two independent decoders; never plays audio"""
import argparse
from pathlib import Path
import subprocess
import hashlib
import struct
import json
import math

parser = argparse.ArgumentParser()
parser.add_argument('output_dir', type=Path)
parser.add_argument('bin_dir', type=Path)
args = parser.parse_args()
p, bin_dir = args.output_dir.resolve(), args.bin_dir.resolve()
rows = []
for n in [48000, 48128, 1024, 0]:
    hashes = []
    for q in [0, 5, 10]:
        f = p / f'encoded-{n}-q{q}.ogg'
        data = f.read_bytes()
        hashes.append(hashlib.sha256(data).hexdigest())
        off = 0
        pages = []
        while off < len(data):
            assert data[off:off+4] == b'OggS'
            segs = data[off+26]
            header = 27 + segs
            body = sum(data[off+27:off+header])
            gp = struct.unpack_from('<q', data, off+6)[0]
            pages.append({'granule': gp, 'flags': data[off+5], 'body': body})
            off += header + body
        if q != 5:
            continue
        sym = subprocess.run([bin_dir / 'readback', f, p / f'symphonia-{n}.f32le'], capture_output=True, text=True, timeout=60)
        (p / f'symphonia-{n}.stdout').write_text(sym.stdout)
        (p / f'symphonia-{n}.stderr').write_text(sym.stderr)
        probe = subprocess.run(['ffprobe', '-v', 'error', '-show_streams', '-of', 'json', str(f)], capture_output=True, text=True, timeout=60)
        (p / f'ffprobe-{n}.json').write_text(probe.stdout)
        (p / f'ffprobe-{n}.stderr').write_text(probe.stderr)
        dest = p / f'decoded-{n}.f32le'
        dec = subprocess.run(['ffmpeg', '-nostdin', '-hide_banner', '-v', 'error', '-y', '-i', str(f), '-map', '0:a:0', '-c:a', 'pcm_f32le', '-f', 'f32le', str(dest)], capture_output=True, text=True, timeout=60)
        (p / f'ffmpeg-{n}.stderr').write_text(dec.stderr)
        rows.append({'source_frames': n, 'ogg_bytes': len(data), 'pages': len(pages), 'last_granule': pages[-1]['granule'], 'last_flags': pages[-1]['flags'], 'ffprobe_exit': probe.returncode, 'ffmpeg_exit': dec.returncode, 'decoded_frames': dest.stat().st_size // 8 if dest.exists() else None, 'stderr': dec.stderr.strip()})
        rows[-1]['symphonia'] = {'exit': sym.returncode, 'metadata': json.loads(sym.stdout) if sym.returncode == 0 else None}
    rows[-1]['q0_q5_q10_bytes_identical'] = len(set(hashes)) == 1
(p / 'results.json').write_text(json.dumps(rows, indent=2))
print(json.dumps(rows, indent=2))

transients = []
for n in (48000, 48128):
    src = list(struct.iter_unpack('<ff', (p / f'source-{n}.f32le').read_bytes()))
    got = list(struct.iter_unpack('<ff', (p / f'decoded-{n}.f32le').read_bytes()))
    def stats(a):
        return {'frames': len(a), 'right_nonzero_frames': sum(r != 0.0 for l, r in a), 'right_peak': max(abs(r) for l, r in a), 'right_rms': math.sqrt(sum(r*r for l, r in a) / len(a)), 'left_peak': max(abs(l) for l, r in a)}
    transients.append({'source': stats(src), 'decoded': stats(got)})
(p / 'transient-results.json').write_text(json.dumps(transients, indent=2))
print(json.dumps(transients, indent=2))
