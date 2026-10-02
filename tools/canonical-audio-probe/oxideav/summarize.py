#!/usr/bin/env python3
"""Summarize saved observations; no encoding, decoding, or playback"""
import argparse
import json
from pathlib import Path
from probe import read_pcm

parser = argparse.ArgumentParser()
parser.add_argument('root',type=Path)
args = parser.parse_args()
root = args.root.resolve()
results = json.loads((root/'results.json').read_text())
results += [json.loads(p.read_text()) for p in sorted(root.glob('result-*.json'))]
rows = []
for result in results:
    row = {k:result.get(k) for k in ['input_frames','quality','signal','status','directory']}
    row['signal'] = row['signal'] or 'tail'
    case = root/result['directory']
    row['resources'] = json.loads((case/'encode.resources.json').read_text())
    if 'ogg' in result:
        row.update({'bytes':result['ogg']['bytes'],'eos_granules':result['ogg']['eos_granules']})
        formats = {}
        for decoder in ['ffmpeg','symphonia']:
            obs = result[decoder]
            if 'pcm' in obs:
                row[decoder] = {'frames':obs['pcm']['frames'],'peak':[c['peak'] for c in obs['pcm']['channels']],'nonfinite':[c['nonfinite'] for c in obs['pcm']['channels']],'snr_db':[c['snr_db_aligned'] for c in obs['pcm']['channels']]}
        streams = result['ffprobe']['metadata'].get('streams',[])
        formats['ffmpeg_48k_stereo'] = len(streams) == 1 and streams[0].get('sample_rate') == '48000' and streams[0].get('channels') == 2
        metadata = result['symphonia'].get('metadata',{})
        formats['symphonia_48k_stereo'] = metadata.get('sample_rate') == 48000 and metadata.get('channels') == 2
        row['decoder_format_checks'] = formats
        row['decoder_agreement_rmse'] = [c['rmse_aligned'] for c in result.get('decoder_agreement',{}).get('channels',[])]
        if not all(formats.values()): row['status'] = 'FAIL_FORMAT'
    rows.append(row)

burst_case = root/'cases/n48000-q0.5-burst'
burst = {}
for decoder in ['input','ffmpeg','symphonia']:
    right = read_pcm(burst_case/f'{decoder}.f32le')[1]
    n = len(right)
    windows = {}
    for label,lo,hi in [('head',0,256),('middle',256,n-256),('tail',n-256,n)]:
        values = right[lo:hi]
        energy = sum(v*v for v in values)
        windows[label] = {'start_frame':lo,'end_frame_exclusive':hi,'energy':energy,'peak':max(map(abs,values),default=0),'peak_frame':lo+max(range(len(values)),key=lambda i:abs(values[i])),'energy_centroid_frame':lo+sum(i*v*v for i,v in enumerate(values))/energy if energy else None}
    burst[decoder] = windows
for decoder in ['ffmpeg','symphonia']:
    for label in ['head','tail']:
        burst[decoder][label]['energy_ratio_to_input'] = burst[decoder][label]['energy']/burst['input'][label]['energy']
        burst[decoder][label]['energy_centroid_shift_frames'] = burst[decoder][label]['energy_centroid_frame']-burst['input'][label]['energy_centroid_frame']
(root/'burst-transients.json').write_text(json.dumps(burst,indent=2)+'\n')
(root/'summary.json').write_text(json.dumps({'cases':rows,'resource_measurement':'Linux wait4 on timeout process tree; max RSS includes short-lived harness inheritance, not codec-only heap','limits':'short cases timeout 90s; additional burst and 64s processes RLIMIT_AS 2 GiB; 64s timeout 600s','admission':'NOT RUN: human listening / gameplay integration / production media admission'},indent=2)+'\n')
for row in rows:
    print(json.dumps(row))
