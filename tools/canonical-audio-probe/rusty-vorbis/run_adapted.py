#!/usr/bin/env python3
"""CoCoBeat priming/mux adaptation experiment, preserving the original API baseline"""
import argparse
import array
from pathlib import Path
import json
import math
import resource
import sys
import time
from run import run, pcm_metrics, ogg_pages, decoder_difference

def check(root, bin_dir, frames, pulse=False, long=False):
    case = root / "adapted-cases" / f"{frames}-q5{'-pulse' if pulse else ''}"
    case.mkdir(parents=True, exist_ok=True)
    encode = [bin_dir/"adapter",case,str(frames),"5"] + (["pulse"] if pulse else [])
    usage_before=resource.getrusage(resource.RUSAGE_CHILDREN)
    started=time.perf_counter()
    commands = [run(encode,case,"encode",seconds=60)]
    if long:
        usage=resource.getrusage(resource.RUSAGE_CHILDREN)
        measured={'wall_seconds':time.perf_counter()-started,'user_cpu_seconds':usage.ru_utime-usage_before.ru_utime,'system_cpu_seconds':usage.ru_stime-usage_before.ru_stime,'max_rss_kib':usage.ru_maxrss,'scope':'Fresh long-stage Python process, first child is encoder; includes input generation/writes, padding, encode, Ogg mux/writes, excludes decoder oracles and compilation','measurement':'Python stdlib resource.RUSAGE_CHILDREN on Linux'}
        (case/'encode-resource.json').write_text(json.dumps(measured,indent=2)+'\n')
    if commands[0]["exit"] != 0:
        return {"frames":frames,"pulse":pulse,"status":"FAIL","commands":commands}
    commands.append(run([bin_dir/"readback",case/"encoded.ogg",case/"symphonia.f32le"],case,"symphonia",seconds=60))
    commands.append(run(["ffprobe","-v","error","-show_streams","-show_format","-of","json",case/"encoded.ogg"],case,"ffprobe"))
    commands.append(run(["ffmpeg","-nostdin","-hide_banner","-v","error","-y","-i",case/"encoded.ogg","-map","0:a:0","-c:a","pcm_f32le","-f","f32le",case/"ffmpeg.f32le"],case,"ffmpeg",seconds=60))
    metrics = {name:pcm_metrics(case/f"{name}.f32le") for name in ["input","symphonia","ffmpeg"]}
    pages = ogg_pages(case/"encoded.ogg")
    packets = [json.loads(line) for line in (case/"packets.jsonl").read_text().splitlines()]
    assert [p['granule'] for p in pages] == [p['adapter_granule'] for p in packets if p['kept']]
    (case/"pages.json").write_text(json.dumps(pages,indent=2)+"\n")
    failures=[]
    formats={}
    for name,index in [("symphonia",1),("ffmpeg",3)]:
        decoded=metrics[name]
        if commands[index]["exit"] != 0 or decoded is None:
            failures.append(f"{name} decode error")
            continue
        if name == 'symphonia':
            formats[name]=json.loads((case/'symphonia.stdout').read_text())
        else:
            metadata=json.loads((case/'ffprobe.stdout').read_text())['streams'][0]
            formats[name]={'sample_rate':int(metadata['sample_rate']),'channels':metadata['channels']}
        if formats[name]['sample_rate'] != 48000 or formats[name]['channels'] != 2:
            failures.append(f'{name} format mismatch')
        if decoded['frames'] != frames:
            failures.append(f"{name} frames {decoded['frames']} != {frames}")
        if any(c['nonfinite'] or c['peak'] > 1 for c in decoded['channel_metrics']):
            failures.append(f'{name} nonfinite or over-unity sample')
        if frames >= 1024:
            ratio=decoded['channel_metrics'][0]['rms']/metrics['input']['channel_metrics'][0]['rms']
            decoded['left_rms_ratio_db']=20*math.log10(max(ratio,1e-30))
            if abs(decoded['left_rms_ratio_db']) > 3:
                failures.append(f'{name} sustained left RMS outside +/-3dB')
    if pages[-1]['granule'] != frames or not pages[-1]['flags']&4:
        failures.append('EOS does not declare original frame count')
    result={'status':'FAIL' if failures else 'PASS','frames':frames,'q':5,'pulse':pulse,'failures':failures,'metrics':metrics,'decoder_formats':formats,'decoder_agreement':decoder_difference(case),'commands':commands,'eos_granule':pages[-1]['granule'],'ogg_bytes':(case/'encoded.ogg').stat().st_size,'adapter_config':json.loads((case/'encode.stdout').read_text())}
    if long:
        result['encoder_resource']=measured
    (case/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({k:result[k] for k in ['status','frames','pulse','failures','eos_granule','ogg_bytes']}),flush=True)
    return result

def pulse_timing(root):
    case=root/'adapted-cases'/'48000-q5-pulse'
    buffers={}
    for name in ['input','symphonia','ffmpeg']:
        data=array.array('f',(case/f'{name}.f32le').read_bytes())
        if sys.byteorder != 'little':
            data.byteswap()
        buffers[name]=data[1::2]
    results={}
    for name in ['symphonia','ffmpeg']:
        results[name]={}
        for label,start,end in [('head',0,512),('tail',48000-512,48000)]:
            source=buffers['input'][start:end]
            target=buffers[name][start:end]
            energy=lambda values:sum(v*v for v in values)
            centroid=lambda values:sum(i*v*v for i,v in enumerate(values))/energy(values)
            def correlation(lag):
                pairs=[(source[i],target[i+lag]) for i in range(len(source)) if 0<=i+lag<len(target)]
                denominator=math.sqrt(sum(a*a for a,b in pairs)*sum(b*b for a,b in pairs))
                return sum(a*b for a,b in pairs)/denominator if denominator else 0
            lag=max(range(-128,129),key=correlation)
            difference_db=10*math.log10(energy(target)/energy(source))
            centroid_shift=centroid(target)-centroid(source)
            value={'energy_ratio_db':difference_db,'peak':max(map(abs,target)),'best_lag_frames':lag,'correlation':correlation(lag),'centroid_shift_frames':centroid_shift}
            value['status']='PASS' if abs(difference_db)<=3 and abs(lag)<=2 and value['correlation']>=0.95 and abs(centroid_shift)<=8 else 'FAIL'
            results[name][label]=value
    result={'status':'PASS' if all(v['status']=='PASS' for parts in results.values() for v in parts.values()) else 'FAIL','gate':'Exploratory timing gate: 512-frame boundary windows; energy +/-3dB, best correlation lag +/-2 frames, correlation >=0.95, energy centroid shift +/-8 frames','metrics':results}
    (case/'pulse-timing.json').write_text(json.dumps(result,indent=2)+'\n')
    return result

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('output_dir', type=Path)
    parser.add_argument('bin_dir', type=Path)
    parser.add_argument('stage', choices=['short', 'pulse', 'long'], nargs='?', default='short')
    args = parser.parse_args()
    root, bin_dir, stage = args.output_dir.resolve(), args.bin_dir.resolve(), args.stage
    if stage in ['pulse','long']:
        summary=json.loads((root/'adapted-results.json').read_text())
        assert all(r['status']=='PASS' for r in summary['cases']), 'Short gate failed'
        if stage=='pulse':
            result=check(root,bin_dir,48000,pulse=True)
            timing=pulse_timing(root) if result['status']=='PASS' else {'status':'NOT RUN'}
            summary['pulse']={'status':'PASS' if result['status']==timing['status']=='PASS' else 'FAIL','decode':result,'timing':timing}
        else:
            assert summary['pulse']['status']=='PASS', 'Pulse gate failed'
            summary['long_64_seconds']=check(root,bin_dir,48000*64,long=True)
        summary['status']='FAIL' if any(summary[key]['status']=='FAIL' for key in ['pulse','long_64_seconds']) else summary['status']
        (root/'adapted-results.json').write_text(json.dumps(summary,indent=2)+'\n')
        return
    assert stage=='short'
    rejection=root/'adapted-cases'/'0-q5'
    rejection.mkdir(parents=True,exist_ok=True)
    zero=run([bin_dir/'adapter',rejection,'0','5'],rejection,'encode')
    assert zero['exit'] != 0, 'Adapter must reject zero-length input'
    results=[check(root,bin_dir,n) for n in [1,1024,1025,48000,48128]]
    summary={'scope':'CoCoBeat priming/mux adaptation experiment, not native API behavior','gate':'Exact frames, 48k stereo, finite samples, peak <=1; sustained left RMS +/-3dB for N>=1024','zero_length':{'status':'PASS','behavior':'Explicitly rejected','command':zero},'cases':results,'pulse':{'status':'NOT RUN'},'long_64_seconds':{'status':'NOT RUN'}}
    summary['status']='PASS' if all(r['status']=='PASS' for r in results) else 'FAIL'
    (root/'adapted-results.json').write_text(json.dumps(summary,indent=2)+'\n')

if __name__=='__main__':
    main()
