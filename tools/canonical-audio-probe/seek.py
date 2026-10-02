"""Compare repeated accurate seeks with existing full decodes; never encode or play"""
import argparse
import array
import csv
import hashlib
import io
import json
import math
from pathlib import Path
import subprocess
import sys


def read_pcm(path):
    raw = path.read_bytes()
    if len(raw) % 8:
        raise ValueError(f'partial stereo frame: {path}')
    values = array.array('f', raw)
    if sys.byteorder != 'little':
        values.byteswap()
    return values


def compare(actual, expected):
    finite = all(math.isfinite(value) for value in actual) and all(math.isfinite(value) for value in expected)
    error = max((abs(a - b) for a, b in zip(actual, expected)), default=0.0) if finite else None
    passed = len(actual) == len(expected) and finite and error <= 1e-6
    return {'status': 'PASS' if passed else 'FAIL', 'frames': len(actual) // 2,
            'expected_frames': len(expected) // 2, 'finite': finite, 'max_abs_error': error}


def main():
    if sys.argv[1:] == ['--self-check']:
        truth = [0.0, 0.5, 0.25, -0.5, 1.0, -1.0]
        assert compare(truth[2:6], [0.25, -0.5, 1.0, -1.0])['status'] == 'PASS'
        assert compare(truth[:4], truth[2:6])['status'] == 'FAIL'
        assert compare(truth[:2], truth[:4])['status'] == 'FAIL'
        assert compare([float('nan'), 0.0], truth[:2])['status'] == 'FAIL'
        print('PASS: absolute-position truth detects one-frame shift, truncation and nonfinite output')
        return
    parser = argparse.ArgumentParser()
    for name in ['seek_bin', 'ogg', 'symphonia_pcm', 'ffmpeg_pcm', 'output_dir']:
        parser.add_argument(name, type=Path)
    parser.add_argument("--preroll-frames", type=int, choices=[0, 1024], default=0)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    references = {name: read_pcm(getattr(args, name + '_pcm')) for name in ['symphonia', 'ffmpeg']}
    total = len(references['symphonia']) // 2
    if total < 4096 or len(references['ffmpeg']) != total * 2:
        raise ValueError('this experiment requires matching complete baselines of at least 4096 stereo frames')
    window = 4096
    targets = [0, total - 1, 1, total - 1025, 1023, 1024, 1025, total // 2,
               total - 1024, total - 1023, total - 257, total - 256, total - 255,
               255, 256, 257, total - 129, total - 128, total - 127, total,
               0, total // 2, 1, total - 1, 0]
    command = [str(args.seek_bin.resolve()), str(args.ogg.resolve()), str(args.output_dir.resolve()), str(window), str(args.preroll_frames), *map(str, targets)]
    result = {'status': 'FAIL', 'window_frames': window, 'preroll_frames': args.preroll_frames, 'eof_semantics': 'target=N is an empty positive sentinel; target<0 or target>N must be explicitly rejected', 'total_frames': total, 'targets': targets,
              'comparison': 'same absolute frame slice, max_abs_error <= 1e-6, exact count, finite values',
              'command': command, 'cases': [],
              'source_sha256': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in
                               [args.ogg, args.symphonia_pcm, args.ffmpeg_pcm, Path(__file__), Path(__file__).with_suffix('.rs')]}}
    try:
        process = subprocess.run(command, capture_output=True, text=True, timeout=60)
        (args.output_dir / 'seek.stdout').write_text(process.stdout)
        (args.output_dir / 'seek.stderr').write_text(process.stderr)
        result['exit_code'] = process.returncode
        if process.returncode:
            result['error'] = process.stderr.strip()
        rows = list(csv.DictReader(io.StringIO(process.stdout)))
        for index, target in enumerate(targets):
            row = rows[index]
            metadata = {key: int(value) for key, value in row.items() if key != 'status' and value}
            if row['status'] != 'PASS':
                result['cases'].append({'status': 'FAIL', 'metadata': metadata, 'error': 'seek API or decode failed; see seek.stderr'})
                continue
            actual = read_pcm(args.output_dir / f'seek-{index}-{target}.f32le')
            expected_frames = min(window, total - target)
            matches = {name: compare(actual, values[target * 2:(target + expected_frames) * 2]) for name, values in references.items()}
            timing = (metadata['index'] == index and metadata['target'] == target
                      and metadata['request'] == metadata['required_ts'] == max(0, target - args.preroll_frames)
                      and metadata['actual_ts'] <= metadata['request'] and metadata['first_packet_frames'] == 0
                      and metadata['output_frames'] == expected_frames
                      and metadata['first_output_frame'] == (target if expected_frames else -1))
            result['cases'].append({'status': 'PASS' if timing and all(m['status'] == 'PASS' for m in matches.values()) else 'FAIL',
                                    'metadata': metadata, 'timing': timing, 'comparisons': matches})
        result['status'] = 'PASS' if process.returncode == 0 and len(rows) == len(targets) and all(c['status'] == 'PASS' for c in result['cases']) else 'FAIL'
    except (subprocess.TimeoutExpired, OSError, ValueError, IndexError, KeyError) as error:
        result['error'] = str(error)
    result['invalid_targets'] = []
    for target, expected_error in [(-1, 'target must be nonnegative'), (total + 1, 'target exceeds declared valid frame count')]:
        invalid_command = command[:4] + [str(args.preroll_frames), str(target)]
        try:
            rejected = subprocess.run(invalid_command, capture_output=True, text=True, timeout=60)
            passed = rejected.returncode != 0 and expected_error in rejected.stderr
            item = {'target': target, 'status': 'PASS_REJECTED' if passed else 'FAIL', 'command': invalid_command, 'exit_code': rejected.returncode, 'error': rejected.stderr.strip()}
        except (subprocess.TimeoutExpired, OSError) as error:
            item = {'target': target, 'status': 'FAIL', 'command': invalid_command, 'error': str(error)}
        result['invalid_targets'].append(item)
        if item['status'] != 'PASS_REJECTED':
            result['status'] = 'FAIL'
    (args.output_dir / 'result.json').write_text(json.dumps(result, indent=2, allow_nan=False) + '\n')
    print(json.dumps({'status': result['status'], 'frames': total, 'seeks': len(result['cases']), 'output': str(args.output_dir)}))
    if result['status'] != 'PASS':
        raise SystemExit(1)


if __name__ == '__main__':
    main()
