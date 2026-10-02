#!/usr/bin/env python3
"""Run the isolated encoder and independent strict decoders without playback"""
import argparse
import array
import hashlib
import json
import math
import os
import resource
from pathlib import Path
import struct
import subprocess
import sys
import time


def run(command, stem, seconds=60, resources=False):
    args = [str(v) for v in command]
    started = time.monotonic()
    with stem.with_suffix('.stdout').open('w') as stdout, stem.with_suffix('.stderr').open('w') as stderr:
        process = subprocess.Popen(['timeout', '--kill-after=5s', f'{seconds}s'] + args, stdout=stdout, stderr=stderr, preexec_fn=lambda: resource.setrlimit(resource.RLIMIT_AS, (2*1024**3,2*1024**3)))
        _, status, usage = os.wait4(process.pid, 0)
        process.returncode = os.waitstatus_to_exitcode(status)
    elapsed = time.monotonic()-started
    if resources:
        stem.with_suffix('.resources.json').write_text(json.dumps({'elapsed_seconds':elapsed,'max_rss_kib':usage.ru_maxrss,'user_seconds':usage.ru_utime,'system_seconds':usage.ru_stime},indent=2)+'\n')
    return {'command': args, 'exit_code': process.returncode, 'wall_seconds': elapsed, 'timeout_seconds': seconds, 'address_space_limit_bytes': 2*1024**3}


def read_pcm(path):
    data = array.array('f')
    data.frombytes(path.read_bytes())
    if sys.byteorder != 'little':
        data.byteswap()
    assert len(data) % 2 == 0, f'partial stereo frame: {path}'
    return [list(data[0::2]), list(data[1::2])]


def magnitude(values):
    finite = [x for x in values if math.isfinite(x)]
    return {'peak': max(map(abs, finite), default=0), 'rms': math.sqrt(sum(x*x for x in finite)/max(1, len(finite))), 'nonfinite': len(values)-len(finite)}


def metrics(path, original):
    pcm = read_pcm(path)
    channels = []
    for values, source in zip(pcm, original):
        overlap = min(len(values), len(source))
        error = sum((values[i]-source[i])**2 for i in range(overlap))
        signal = sum(source[i]**2 for i in range(overlap))
        item = magnitude(values)
        item.update({'first_128': magnitude(values[:128]), 'last_128': magnitude(values[-128:]), 'first_8': [x if math.isfinite(x) else None for x in values[:8]], 'last_8': [x if math.isfinite(x) else None for x in values[-8:]], 'overlap_frames': overlap, 'rmse_aligned': math.sqrt(error/max(1,overlap)) if math.isfinite(error) else None, 'snr_db_aligned': 10*math.log10(signal/error) if math.isfinite(signal) and math.isfinite(error) and signal > 0 and error > 0 else None, 'before_input_tail_128': magnitude(values[:max(0,len(source)-128)]), 'input_tail_128': magnitude(values[max(0,len(source)-128):len(source)])})
        channels.append(item)
    return {'frames': len(pcm[0]), 'channels': channels}


def ogg(path):
    data = path.read_bytes()
    pages, offset, packet, packets = [], 0, bytearray(), []
    while offset < len(data):
        assert data[offset:offset+4] == b'OggS', f'bad page at {offset}'
        count = data[offset+26]
        lacing = data[offset+27:offset+27+count]
        end = offset+27+count+sum(lacing)
        assert end <= len(data), 'truncated page'
        page = bytearray(data[offset:end])
        stored = struct.unpack_from('<I', page, 22)[0]
        page[22:26] = b'\0'*4
        crc = 0
        for byte in page:
            crc ^= byte << 24
            for _ in range(8):
                crc = ((crc << 1) ^ (0x04C11DB7 if crc & 0x80000000 else 0)) & 0xffffffff
        assert crc == stored, f'CRC mismatch at {offset}'
        granule, serial, sequence = struct.unpack_from('<QII', data, offset+6)
        pages.append({'offset': offset, 'bytes': end-offset, 'flags': data[offset+5], 'granule': granule, 'serial': serial, 'sequence': sequence, 'crc_valid': True})
        cursor = offset+27+count
        for size in lacing:
            packet.extend(data[cursor:cursor+size])
            cursor += size
            if size < 255:
                packets.append(bytes(packet))
                packet.clear()
        offset = end
    assert not packet, 'unfinished packet'
    assert len(packets) >= 3 and packets[0][:7] == b'\x01vorbis', 'missing Vorbis identification'
    return {'pages': pages, 'packets': len(packets), 'channels': packets[0][11], 'sample_rate': struct.unpack_from('<I',packets[0],12)[0], 'eos_granules': [p['granule'] for p in pages if p['flags'] & 4], 'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}


def case(root, bin_dir, n, quality, signal='tail', encode_seconds=90):
    dest = root / 'cases' / (f'n{n}-q{quality}' + (f'-{signal}' if signal != 'tail' else ''))
    dest.mkdir(parents=True, exist_ok=True)
    result = {'input_frames': n, 'quality': quality, 'signal': signal, 'directory': str(dest.relative_to(root))}
    result['encode'] = run([bin_dir/'cocobeat-oxideav-probe',dest,n,quality,signal],dest/'encode',seconds=encode_seconds,resources=True)
    if result['encode']['exit_code']:
        result['encode']['error'] = (dest/'encode.stderr').read_text()
        result['status'] = 'PASS_EMPTY_REJECTED' if n == 0 and 'BadPcmShape' in result['encode']['error'] else 'FAIL_ENCODE'
        return result
    result['ogg'] = ogg(dest/'encoded.ogg')
    source = read_pcm(dest/'input.f32le')
    result['input'] = metrics(dest/'input.f32le', source)
    result['ffprobe'] = run(['ffprobe','-v','error','-show_streams','-of','json',dest/'encoded.ogg'],dest/'ffprobe')
    result['ffprobe']['metadata'] = json.loads((dest/'ffprobe.stdout').read_text())
    result['ffmpeg'] = run(['ffmpeg','-nostdin','-hide_banner','-loglevel','warning','-xerror','-err_detect','explode','-c:a','vorbis','-i',dest/'encoded.ogg','-map','0:a:0','-vn','-c:a','pcm_f32le','-f','f32le','-y',dest/'ffmpeg.f32le'],dest/'ffmpeg')
    result['symphonia'] = run([bin_dir/'readback',dest/'encoded.ogg',dest/'symphonia.f32le'],dest/'symphonia')
    for decoder in ['ffmpeg','symphonia']:
        raw = dest/f'{decoder}.f32le'
        if raw.exists():
            result[decoder]['pcm'] = metrics(raw,source)
        if decoder == 'symphonia' and result[decoder]['exit_code'] == 0:
            result[decoder]['metadata'] = json.loads((dest/'symphonia.stdout').read_text())
    a, b = (dest/'ffmpeg.f32le'), (dest/'symphonia.f32le')
    if a.exists() and b.exists():
        result['decoder_agreement'] = metrics(b,read_pcm(a))
    result['checks'] = {'ogg_eos_frames': result['ogg']['eos_granules'] == [n], 'ogg_format': result['ogg']['channels'] == 2 and result['ogg']['sample_rate'] == 48000}
    streams = result['ffprobe']['metadata'].get('streams',[])
    result['checks']['ffmpeg_format'] = len(streams) == 1 and streams[0].get('sample_rate') == '48000' and streams[0].get('channels') == 2
    metadata = result['symphonia'].get('metadata',{})
    result['checks']['symphonia_format'] = metadata.get('sample_rate') == 48000 and metadata.get('channels') == 2
    for decoder in ['ffmpeg','symphonia']:
        d = result[decoder]
        result['checks'][decoder+'_exit'] = d['exit_code'] == 0
        result['checks'][decoder+'_frames'] = d.get('pcm',{}).get('frames') == n
        result['checks'][decoder+'_finite'] = 'pcm' in d and all(c['nonfinite'] == 0 for c in d['pcm']['channels'])
    result['status'] = 'PASS_STRUCTURAL' if all(result['checks'].values()) else 'FAIL_STRUCTURAL'
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('root',type=Path)
    parser.add_argument('bin_dir',type=Path)
    parser.add_argument('--single',type=int)
    parser.add_argument('--quality',type=float,default=.5)
    parser.add_argument('--signal',choices=['tail','burst','near-full'],default='tail')
    parser.add_argument('--encode-seconds',type=int,default=90)
    args = parser.parse_args()
    root = args.root.resolve()
    bin_dir = args.bin_dir.resolve()
    if args.single is not None:
        result = case(root,bin_dir,args.single,args.quality,args.signal,args.encode_seconds)
        filename = f'result-n{args.single}-q{args.quality}-{args.signal}.json'
        (root/filename).write_text(json.dumps(result,indent=2,allow_nan=False)+'\n')
        print(json.dumps({k:result[k] for k in ['input_frames','quality','status']}),flush=True)
        return
    results = []
    for n,q in [(0,.5),(1,.5),(1024,.5),(1025,.5),(48000,0.0),(48000,.5),(48000,1.0),(48128,.5)]:
        result = case(root,bin_dir,n,q)
        results.append(result)
        (root/'results.json').write_text(json.dumps(results,indent=2,allow_nan=False)+'\n')
        summary = {k:result[k] for k in ['input_frames','quality','status']}
        summary.update({d:result.get(d,{}).get('pcm',{}).get('frames') for d in ['ffmpeg','symphonia']})
        print(json.dumps(summary),flush=True)
    qualities = [r for r in results if r['input_frames'] == 48000 and 'ogg' in r]
    (root/'quality-comparison.json').write_text(json.dumps({'settings':[{'quality':r['quality'],'bytes':r['ogg']['bytes'],'sha256':r['ogg']['sha256']} for r in qualities], 'distinct_stream_bytes': len({r['ogg']['sha256'] for r in qualities}) == 3, 'meaning':'oxideav-vorbis quality in [0,1], no libvorbis quality equivalence claimed'},indent=2)+'\n')


if __name__ == '__main__':
    main()
