// Native GPU frames with matching offline Kira audio, not a hardware recording
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {spawn} = require('node:child_process');

const args = process.argv.slice(2);
if (args.length !== 5) throw Error('motion.cjs GAME RUNTIME_TEST PACKAGE WORLD NEW_OUTPUT');
const [binary, mixer, packageDir] = args.slice(0, 3).map(p => path.resolve(p));
const world = args[3], output = path.resolve(args[4]);
if (!['neon', 'forest', 'candy', 'star-sea'].includes(world)) throw Error('Unknown world');
fs.mkdirSync(output);
const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const receipt = {
  scope: 'Native Bevy GPU images and production DuoEngine synthetic controls, muxed with offline native Kira Renderer audio; no physical input or speaker capture',
  binary_sha256: sha(binary), mixer_sha256: sha(mixer), world, package: packageDir, commands: [],
};
const save = () => fs.writeFileSync(path.join(output, 'receipt.json'), JSON.stringify(receipt, null, 2));
async function run(name, command, argv, env = {}) {
  const log = fs.openSync(path.join(output, `${name}.log`), 'wx');
  const child = spawn(command, argv, {env: {...process.env, ...env}, stdio: ['ignore', log, log], detached: true});
  const timer = setTimeout(() => {if (child.pid) try {process.kill(-child.pid, 'SIGKILL');} catch (error) {if (error.code !== 'ESRCH') throw error;}}, 900000);
  let code;
  try {
    code = await new Promise((resolve, reject) => {child.on('error', reject); child.on('exit', resolve);});
  } finally {
    clearTimeout(timer); fs.closeSync(log);
  }
  receipt.commands.push({command, args: argv, env, exit: code}); save();
  if (code !== 0) throw Error(`${name} exited ${code}`);
}
(async () => {
  const frames = path.join(output, 'frames'), audio = path.join(output, 'audio');
  await run('render', binary, ['--package', packageDir, '--presentation-motion-smoke', world, frames]);
  const capture = JSON.parse(fs.readFileSync(path.join(frames, 'core-events.json')));
  const images = fs.readdirSync(frames).filter(name => /^frame_\d{4}\.png$/.test(name)).sort();
  if (images.length !== capture.sampled_frames) throw Error('Incomplete frame sequence');
  const samples = JSON.parse(fs.readFileSync(path.join(frames, 'frames.json')));
  if (samples.length !== images.length || samples.some((sample, i) =>
    sample.sample !== i || sample.file !== images[i] || sample.requested_song_frames !== i * 1600 ||
    !Number.isSafeInteger(sample.requested_update) || !Number.isSafeInteger(sample.prepared_update) ||
    !Number.isFinite(sample.song_seconds) ||
    sample.saved_song_frames !== sample.requested_song_frames || sample.requested_update - sample.prepared_update < 2 ||
    Math.abs(sample.song_seconds - i / 30) > 1e-9)) throw Error('Screenshot time does not match audio timeline');
  receipt.sample_times_sha256 = sha(path.join(frames, 'frames.json'));
  const frameHashes = {};
  for (let i = 0; i < images.length; i++) {
    if (images[i] !== `frame_${String(i).padStart(4, '0')}.png`) throw Error('Frame gap');
    frameHashes[images[i]] = sha(path.join(frames, images[i]));
  }
  fs.writeFileSync(path.join(output, 'frame-hashes.json'), JSON.stringify(frameHashes, null, 2));
  receipt.events_sha256 = sha(path.join(frames, 'events.json'));
  await run('audio', mixer, ['feedback_audio::tests::export_reference_mix', '--ignored', '--exact', '--nocapture'], {
    COCOBEAT_FEEDBACK_PREVIEW_SOURCE: packageDir,
    COCOBEAT_FEEDBACK_PREVIEW_EVENTS: path.join(frames, 'events.json'),
    COCOBEAT_FEEDBACK_PREVIEW_OUT: audio,
  });
  const video = path.join(output, 'preview.mp4');
  await run('encode', 'ffmpeg', ['-nostdin', '-v', 'error', '-framerate', '30', '-i', path.join(frames, 'frame_%04d.png'), '-i', path.join(audio, 'mixed.wav'), '-t', String(capture.duration_seconds), '-c:v', 'libx264', '-threads', '2', '-preset', 'fast', '-crf', '20', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-b:a', '192k', '-movflags', '+faststart', video]);
  if (sha(binary) !== receipt.binary_sha256 || sha(mixer) !== receipt.mixer_sha256) throw Error('Input binary changed');
  const mix = JSON.parse(fs.readFileSync(path.join(audio, 'receipt.json')));
  if (mix.status !== 'PASS' || mix.source_kind !== 'canonical_package' || mix.content_id !== capture.content_id) throw Error('Audio does not match captured content');
  if (sha(path.join(frames, 'events.json')) !== receipt.events_sha256 || sha(path.join(audio, 'events.json')) !== receipt.events_sha256) throw Error('Audio event identity changed');
  receipt.status = 'PASS_NATIVE_FRAMES_OFFLINE_MIX';
  receipt.frames = images.length; receipt.duration_seconds = capture.duration_seconds;
  receipt.video_sha256 = sha(video); receipt.content_id = capture.content_id;
  receipt.audio = mix; save();
  console.log(JSON.stringify(receipt));
})().catch(error => {receipt.status = 'FAIL'; receipt.error = String(error); save(); console.error(error); process.exitCode = 1;});
