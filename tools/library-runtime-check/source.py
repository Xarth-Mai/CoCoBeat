"""One or more owned default-entry Ready source UI observations; not hardware acceptance."""
import argparse
import csv
import importlib.util
import json
import math
import os
from pathlib import Path
import signal
import struct
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.dont_write_bytecode = True
SUPERVISOR = ROOT / 'tools/library-runtime-check/check.py'
spec = importlib.util.spec_from_file_location('library_check', SUPERVISOR)
common = importlib.util.module_from_spec(spec)
spec.loader.exec_module(common)
SCENARIOS = ('source-complete', 'source-back', 'source-focus', 'source-bad-json', 'source-unknown-rules', 'source-background-error', 'source-close')
OBJECTS = common.OBJECTS
SOURCE_SHA = '3390dd080cb536fd4a598ea933618dd99b4bf697c0b2874ff5cb0e220feb09e8'
AUTHORING_SHA = '7eb6b8ad5971367e37099dec41b9729c98523895fe70f73df4a61a618281637e'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('game', 'receipt', 'lab', 'lab-receipt', 'source', 'authoring', 'output'):
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--scenario', choices=SCENARIOS, action='append')
    args = parser.parse_args()
    for name in ('game', 'receipt', 'lab', 'lab_receipt', 'source', 'authoring', 'output'):
        setattr(args, name, getattr(args, name).resolve())
    assert common.digest(args.source) == SOURCE_SHA
    assert common.digest(args.authoring) == AUTHORING_SHA
    binaries = {'game': common.identity(args.game, args.receipt), 'lab': common.identity(args.lab, args.lab_receipt)}
    protected = {str(path): common.digest(path) for path in (args.game, args.receipt, args.lab, args.lab_receipt, args.source, args.authoring, SUPERVISOR, Path(__file__).resolve())}
    args.output.mkdir()
    result = {'status': 'RUNNING', 'binaries': binaries, 'protected_inputs': protected, 'cases': [],
              'scope': 'Actual default game process, full intro and original keyboard-message capture, paired imports menu, source worker / four objects / Ready / Kira cursor / duet Replay',
              'not_run': ['physical keyboard/gamepad/mixed input', 'speaker or DAC timing', 'release performance admission', 'four-platform native source UI', 'human/mother-tongue acceptance'], 'visual_review': 'NOT RUN'}
    common.save(args.output / 'summary.json', result)

    def unchanged():
        assert all(common.digest(Path(path)) == sha for path, sha in protected.items())

    try:
        for scenario in args.scenario or SCENARIOS:
            unchanged()
            case = args.output / scenario
            case.mkdir()
            cwd, data, observation, child_dir = (case / name for name in ('cwd', 'data', 'observation', 'child'))
            cwd.mkdir(); data.mkdir(); child_dir.mkdir()
            library = data / 'cocobeat/songs'
            imports = library / 'imports'
            imports.mkdir(parents=True)
            source, authoring = imports / '00-source.wav', imports / '00-source.authoring.json'
            source.write_bytes(args.source.read_bytes())
            if scenario == 'source-bad-json':
                authoring.write_text('{ invalid owned authoring')
            else:
                document = json.loads(args.authoring.read_text())
                if scenario in ('source-unknown-rules', 'source-background-error'): document['ruleset_id'] = 'unknown-production-ruleset'
                authoring.write_text(json.dumps(document, ensure_ascii=False, indent=2) + '\n')
            copied = {str(path): common.digest(path) for path in (source, authoring)}
            env = {key: value for key, value in os.environ.items() if not key.startswith('COCOBEAT_')}
            env.pop('ENABLE_GAMESCOPE_WSI', None)
            env.update({'DISABLE_GAMESCOPE_WSI': '1', 'XDG_CONFIG_HOME': str(case / 'config'), 'XDG_DATA_HOME': str(data),
                        'COCOBEAT_LIBRARY_OBSERVATION_DIR': str(observation), 'COCOBEAT_LIBRARY_OBSERVATION_SIZE': '1280x800',
                        'COCOBEAT_LIBRARY_OBSERVATION_LOCALE': 'zh-CN', 'COCOBEAT_LIBRARY_OBSERVATION_SCENARIO': scenario})
            command = ['gamescope', '--backend', 'headless', '--expose-wayland', '-W', '1280', '-H', '800', '-r', '60', '--',
                       sys.executable, str(SUPERVISOR), '--child', str(child_dir), str(args.game)]
            item = {'scenario': scenario, 'command': command, 'cwd': str(cwd), 'library': str(library), 'fixture_sha256': copied,
                    'environment': {name: env[name] for name in ('XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'COCOBEAT_LIBRARY_OBSERVATION_SCENARIO', 'DISABLE_GAMESCOPE_WSI')}, 'checks': {}}
            result['cases'].append(item)
            common.save(args.output / 'summary.json', result)
            with (case / 'stdout').open('xb') as out, (case / 'stderr').open('xb') as err:
                process = subprocess.Popen(command, cwd=cwd, env=env, stdout=out, stderr=err, start_new_session=True)
                item['wrapper_pid'] = process.pid
                common.save(args.output / 'summary.json', result)
                try: process.wait(timeout=180)
                finally:
                    if process.poll() is None:
                        os.killpg(process.pid, signal.SIGTERM)
                        try: process.wait(timeout=5)
                        except subprocess.TimeoutExpired: os.killpg(process.pid, signal.SIGKILL); process.wait()
            launch = json.loads((child_dir / 'game-launch.json').read_text())
            exited = json.loads((child_dir / 'child-exit.json').read_text())
            item.update({'wrapper_exit_code': process.returncode, 'actual_game_launch': launch, 'actual_game_exit': exited})
            assert process.returncode == 0 and exited['returncode'] == 0 and launch['game_pid'] == exited['game_pid']
            assert launch['command'] == [str(args.game)] and launch['cwd'] == str(cwd), 'Use the default game entry, never hidden --package / --import-authored / --library arguments'
            assert launch['environment']['DISABLE_GAMESCOPE_WSI'] == '1'
            assert launch['environment']['XDG_DATA_HOME'] == str(data)
            assert not any('[Gamescope WSI]' in line or 'error 4' in line.lower() for line in (child_dir / 'game.stderr').read_text(errors='replace').splitlines())
            report = json.loads((observation / 'result.json').read_text())
            metadata = json.loads((observation / 'metadata.json').read_text())
            assert report['status'] == 'PASS' and report['scenario'] == scenario and report['owned_workers_finished']
            assert report['process_id'] == metadata['process_id'] == launch['game_pid']
            assert report['capture'] and all(value == {'Ok': [1280, 800]} for value in report['capture'].values())
            for name in report['capture']:
                with (observation / f'{name}.png').open('rb') as image:
                    header = image.read(24)
                assert header[:8] == b'\x89PNG\r\n\x1a\n' and struct.unpack('>II', header[16:24]) == (1280, 800)
            item['checks']['native_screenshot_callback_and_png_dimensions'] = 'PASS'
            with (observation / 'frames.csv').open() as frames:
                rows = list(csv.DictReader(frames))
            ready_controls = [row for row in rows if row['phase'] == 'Ready' and row['controls_enabled'] == 'true']
            assert ready_controls and all(float(row['brand_seconds']) >= 6.60 for row in ready_controls)
            assert all(float(row['brand_seconds']) >= 6.60 for row in rows if row['phase'] == 'Running')
            records = report['source_records']
            confirmations = [row for row in records if row['label'] == 'actual-source-confirmation']
            assert confirmations and all(row['source'] == str(source) and row['authoring'] == str(authoring) and not row['destination_exists_before_confirmation'] for row in confirmations)
            destination = Path(confirmations[0]['destination'])
            assert destination.parent == library and all(row['destination'] == str(destination) for row in confirmations)
            if destination.exists():
                assert {path.name for path in destination.iterdir()} == set(OBJECTS)
                item['package'] = {'path': str(destination), 'objects_sha256': {name: common.digest(destination / name) for name in OBJECTS}}
                manifest = (destination / 'song.package').read_bytes()
                assert b'cocobeat-game/' in manifest and b'aotuv-lancer-vorbis-q10-v1' in manifest and b'cocobeat-lab/' not in manifest
                invocation = [str(args.lab), 'verify-package', str(destination)]
                verified = subprocess.run(invocation, capture_output=True, timeout=90)
                (case / 'verify.stdout').write_bytes(verified.stdout); (case / 'verify.stderr').write_bytes(verified.stderr)
                item['package_verify'] = {'command': invocation, 'exit_code': verified.returncode}
                assert verified.returncode == 0
            if scenario == 'source-complete':
                ready, running, paused = (report['snapshots'][name] for name in ('imported_ready', 'imported_running', 'imported_paused'))
                assert ready['phase'] == 'Ready' and not ready['audio_started'] and ready['source_position_seconds'] is None
                assert ready['package_path'] == str(destination) and ready['canonical_frames'] == 3072000
                assert ready['content_id'] != report['snapshots']['before_import']['content_id']
                ready_rows = [row for row in rows if row['content_id'] == ready['content_id'] and row['phase'] == 'Ready']
                assert ready_rows and all(not row['audio_position'] for row in ready_rows)
                assert running['phase'] == 'Running' and running['source_position_seconds'] >= 0.5 and running['acknowledged_frame'] > 0
                assert paused['phase'] == 'Paused' and paused['content_id'] == ready['content_id']
                diagnostics = next(row['captures'] for row in records if row['label'] == 'actual-hit-diagnostics')
                assert len(diagnostics) == 2 and {row['player'] for row in diagnostics} == {'P1', 'P2'}
                assert all(row['consumed_ns'] >= row['observed_ns'] and row['seq'] == 0 for row in diagnostics)
                replay = json.loads(bytes(paused['replay_bytes']))
                hits = [fact for fact in replay['facts'] if fact['type'] == 'hit']
                assert len(hits) == 2 and {fact['player'] for fact in hits} == {1, 2}
                assert replay['content_id'] == ready['content_id']
                histories = [json.loads(path.read_text()) for path in (cwd / 'replays').glob('*.json')]
                assert any(history['content_id'] == replay['content_id'] and history['facts'] == replay['facts'] for history in histories)
                pcm = case / 'readback.f32le'
                invocation = [str(args.lab), 'readback-canonical', str(destination / 'song.audio.ogg'), '3072000', str(pcm)]
                readback = subprocess.run(invocation, capture_output=True, timeout=90)
                (case / 'readback.stdout').write_bytes(readback.stdout); (case / 'readback.stderr').write_bytes(readback.stderr)
                item['readback'] = {'command': invocation, 'exit_code': readback.returncode}
                assert readback.returncode == 0 and pcm.stat().st_size == 3072000 * 8
                with pcm.open('rb') as stream:
                    while block := stream.read(65536): assert all(math.isfinite(value[0]) for value in struct.iter_unpack('<f', block))
                item['checks']['fresh_ready_then_actual_duet'] = 'PASS'
            elif scenario in ('source-bad-json', 'source-unknown-rules'):
                rejection = next(row for row in records if row['label'] == 'actual-source-rejection')
                assert rejection['notice_key'] == 'library.failed' and rejection['notice_args']
                assert common.song_state(report['snapshots']['before_import']) == common.song_state(report['snapshots']['rejected_source'])
                if scenario == 'source-bad-json': assert not destination.exists() and 'Invalid authoring document' in json.dumps(rejection['notice_args'])
                else: assert destination.exists() and 'Unsupported song ruleset' in json.dumps(rejection['notice_args'])
                item['checks']['original_song_and_actual_error_preserved'] = 'PASS'
            elif scenario in ('source-back', 'source-focus', 'source-background-error'):
                reopened = [row for row in records if row['label'] == 'reopened-library-never-selected-abandoned-result']
                if reopened:
                    assert common.song_state(report['snapshots']['before_import']) == common.song_state(report['snapshots']['after_background'])
                    item['checks']['return_focus_reopen_did_not_select_result'] = 'PASS'
                else: item['checks']['return_focus_reopen_did_not_select_result'] = 'NOT RUN'
                actual_reopen = [row for row in records if row['label'] == 'actual-reopened-library']
                item['checks']['reopen_during_actual_unfinished_import'] = 'PASS' if reopened and any(row['importing'] and not row['worker_finished'] for row in actual_reopen) else 'NOT RUN'
                probe = [row for row in records if row['label'] == 'emit-ready-confirm-while-import-busy']
                if probe:
                    assert all(row['importing'] and not row['worker_finished'] for row in probe)
                    assert any(row['label'] == 'busy-confirm-did-not-start-song' and row['phase'] == 'Ready' for row in records)
                    item['checks']['ready_confirm_during_actual_unfinished_import'] = 'PASS'
                else: item['checks']['ready_confirm_during_actual_unfinished_import'] = 'NOT RUN'
                if scenario == 'source-background-error':
                    abandoned = [row for row in records if row['label'] == 'selection-abandoned-before-reopen' and not row['busy']]
                    if abandoned:
                        assert abandoned[-1]['notice_key'] == 'library.failed' and 'Unsupported song ruleset' in json.dumps(abandoned[-1]['notice_args'])
                        assert destination.exists()
                        item['checks']['background_actual_error_preserved'] = 'PASS'
                    else: item['checks']['background_actual_error_preserved'] = 'NOT RUN'
            elif scenario == 'source-close':
                item['checks']['close_with_owned_worker_joined'] = 'PASS' if report['close_requested_while_owned'] else 'NOT RUN'
                item['checks']['close_with_actual_unfinished_encoding'] = 'PASS' if report['worker_unfinished_at_close'] else 'NOT RUN'
            assert all(common.digest(Path(path)) == sha for path, sha in copied.items())
            assert not list(library.glob('.cocobeat-*')), 'Returned-error owned staging leaked'
            item.update({'report': report, 'metadata': metadata, 'raw_sha256': {str(path.relative_to(case)): common.digest(path) for path in case.rglob('*') if path.is_file()}})
            common.save(args.output / 'summary.json', result)
            unchanged()
        result['status'] = 'PASS_SCOPED_SOURCE_UI_OBSERVATIONS'
    except BaseException as error:
        result.update({'status': 'FAIL', 'error': repr(error)})
        raise
    finally:
        result['protected_inputs_unchanged'] = all(common.digest(Path(path)) == sha for path, sha in protected.items())
        common.save(args.output / 'summary.json', result)
    print(json.dumps({'status': result['status'], 'summary': str(args.output / 'summary.json')}))


if __name__ == '__main__': main()
