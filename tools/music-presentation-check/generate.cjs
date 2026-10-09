// Original CoCoBeat presentation control; generated audio CC0-1.0
// Development fixture only, outside the product's import/runtime path
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const rate = 48000, seconds = 80, frames = rate * seconds, beat = rate / 2;
const out = path.resolve(process.argv[2] || "target/music-presentation-reference");
fs.mkdirSync(out, {recursive: false});
const sine = Int16Array.from({length: 4096}, (_, i) => Math.round(Math.sin(i * 2 * Math.PI / 4096) * 32767));
const frequency = Array.from({length: 128}, (_, midi) => Math.round(440000 * 2 ** ((midi - 69) / 12)));
const osc = (frame, midi, harmonic = 1) => sine[Math.floor((frame * frequency[midi] * harmonic % 48000000) * 4096 / 48000000)] / 32768;
const noise = frame => { let n = Math.imul(frame + 1, 747796405) + 2891336453 | 0; n ^= n >>> 16; return (n & 65535) / 32768 - 1; };
const envelope = (frame, length, attack = 240) => frame >= length ? 0 : Math.min(1, frame / attack) * (1 - frame / length) ** 2;
const progressions = [
  [[48,52,55],[45,48,52],[41,45,48],[43,47,50],[48,52,55]],
  [[50,53,57],[46,50,53],[53,57,60],[48,52,55],[50,53,57]],
  [[55,59,62],[52,55,59],[48,52,55],[50,54,57],[55,59,62]],
  [[45,48,52],[41,45,48],[48,52,55],[43,47,50],[45,48,52]],
];
const names = ['electronic-city','woodland-plucks','toy-pop','glass-ambient'];
const wav = Buffer.alloc(44 + frames * 4);
wav.write('RIFF'); wav.writeUInt32LE(wav.length - 8, 4); wav.write('WAVEfmt ', 8);
wav.writeUInt32LE(16,16); wav.writeUInt16LE(1,20); wav.writeUInt16LE(2,22);
wav.writeUInt32LE(rate,24); wav.writeUInt32LE(rate * 4,28); wav.writeUInt16LE(4,32); wav.writeUInt16LE(16,34);
wav.write('data',36); wav.writeUInt32LE(frames * 4,40);
let peak = 0, energy = [0,0,0,0];
for (let frame = 0; frame < frames; frame++) {
  const second = frame / rate, section = Math.floor(second / 20), local = frame % (rate * 20);
  const chord = progressions[section][Math.floor(local / (rate * 2)) % 5];
  const pulse = frame % beat, dense = second >= 48 && second < 56, step = dense ? beat / 4 : beat / 2;
  const pluck = frame % step, note = chord[Math.floor(frame / step) % 3] + 12;
  let left = 0, right = 0;
  if (!(second >= 68 && second < 70)) {
    const low = osc(pulse, chord[0] - 12) * envelope(pulse, beat) * 0.09;
    let tone, percussion;
    if (section === 0) {
      tone = (osc(pluck,note) + osc(pluck,note,2) * 0.24 + osc(pluck,note,3) * 0.1) * envelope(pluck,step) * 0.14;
      percussion = osc(pulse,31) * envelope(pulse,7000,80) * 0.2 + noise(frame) * envelope(frame % (beat/2),1300,10) * 0.045;
    } else if (section === 1) {
      tone = (osc(pluck,note) + osc(pluck,note,3) * 0.16) * envelope(pluck,step * 0.9,100) * 0.16;
      percussion = osc(pulse,77) * envelope(pulse,2200,40) * 0.055;
    } else if (section === 2) {
      tone = (osc(pluck,note) + osc(pluck,note,2) * 0.3) * envelope(pluck,step * 0.8,100) * 0.13;
      percussion = noise(frame) * envelope(pulse,2300,10) * (Math.floor(frame/beat)%2 ? 0.075 : 0.035);
    } else {
      const bell = frame % (beat * 2), bellNote = chord[Math.floor(frame/(beat*2)) % 3] + 12;
      tone = (osc(bell,bellNote) + osc(bell,bellNote,2) * 0.3 + osc(bell,bellNote,4) * 0.12) * envelope(bell,beat*2,600) * 0.14;
      percussion = 0;
    }
    const chordAge = local % (rate * 2);
    const pad = chord.reduce((sum,midi) => sum + osc(frame,midi), 0) * envelope(chordAge,rate*2,2400) * (section === 3 ? 0.032 : 0.018);
    const quiet = second >= 32 && second < 36 ? 0.13 : 1;
    const fade = Math.min(1, frame / 4800, (frames - frame - 1) / (rate * 2));
    left = (low + tone * 0.9 + pad + percussion) * quiet * fade;
    right = (low + tone * 0.75 + pad * 1.1 - percussion * 0.55) * quiet * fade;
  }
  peak = Math.max(peak,Math.abs(left),Math.abs(right));
  energy[section] += (left*left + right*right)/2;
  assert(Math.abs(left)<0.95 && Math.abs(right)<0.95);
  wav.writeInt16LE(Math.round(left * 32767),44 + frame*4);
  wav.writeInt16LE(Math.round(right * 32767),46 + frame*4);
}
const timeline = {license:'CC0-1.0', sample_rate:rate,frames,bpm:120,scope:'Original constructed presentation control, not commercial music quality or independent MIR truth',sections:names.map((name,i)=>({name,start_frame:i*20*rate,end_frame:(i+1)*20*rate,progression:progressions[i],chord_frames:rate*2})),controls:[{start_frame:32*rate,end_frame:36*rate,label:'quiet'},{start_frame:48*rate,end_frame:56*rate,label:'dense sixteenth notes'},{start_frame:68*rate,end_frame:70*rate,label:'exact silence'},{start_frame:78*rate,end_frame:80*rate,label:'ending fade'}]};
const authoring = {schema_version:1,song_id:'original-presentation-80s',ruleset_id:'duo-watermark-v1',source_note:'Original CC0 synthesized four-timbre control; generator generate.cjs; manually authored final 48 kHz frame coordinates',anchors:[6,10,14,18,24,28,34,38,44,48,50,52,54,58,64,68,72,76,78].map((sec,i)=>({id:i+1,frame:sec*rate})),sections:timeline.sections.map((s,i)=>({id:i+1,start_frame:s.start_frame,end_frame:s.end_frame,label:s.name}))};
const hash = value => crypto.createHash('sha256').update(value).digest('hex');
const json = value => JSON.stringify(value,null,2)+'\n';
assert.equal(wav.readUInt32LE(40),15360000);
assert(wav.subarray(44+68*rate*4,44+70*rate*4).every(n=>n===0));
const events = [];
for (let half = 4; half < 158; half++) {
  events.push({time_seconds:half/2,kind:'hit',player:half%2 ? 'P2' : 'P1'});
}
for (const anchor of authoring.anchors) {
  events.push({time_seconds:anchor.frame/rate+0.01,kind:'hit',player:'P2'});
  events.push({time_seconds:anchor.frame/rate+0.04,kind:'duo',precise:anchor.id%3!==0});
}
events.sort((a,b)=>a.time_seconds-b.time_seconds);
events.forEach((event,i)=>event.event_id=i+1);
const preview = {scope:'Authored offline audio audition events, not captured core or physical input',sample_rate:rate,events};
const outputs = {'original-80s.wav':wav,'authoring.json':json(authoring),'timeline.json':json(timeline),'preview-events.json':json(preview)};
for (const [name,bytes] of Object.entries(outputs)) fs.writeFileSync(path.join(out,name),bytes);
const receipt = {generator_sha256:hash(fs.readFileSync(__filename)),files:Object.fromEntries(Object.entries(outputs).map(([name,bytes])=>[name,hash(bytes)])),frames,peak,rms_by_section:energy.map(sum=>Math.sqrt(sum/(20*rate))),checks:['80 s PCM16 stereo 48 kHz','no clipping','68–70 s exact silence'],listening_review:'NOT RUN'};
fs.writeFileSync(path.join(out,'receipt.json'),json(receipt));
console.log(json(receipt));
