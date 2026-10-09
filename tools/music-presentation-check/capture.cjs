// Native deterministic GPU captures; audio and device acceptance are recorded separately
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const {spawn} = require('node:child_process');

const [binary, packageDir, destination] = process.argv.slice(2).map(p => path.resolve(p));
if (!binary || !packageDir || !destination) throw Error('capture.cjs BINARY PACKAGE NEW_OUTPUT');
fs.mkdirSync(destination);
const frozen = path.join(destination, 'cocobeat-game');
fs.copyFileSync(binary, frozen);
fs.chmodSync(frozen, 0o755);
const sha = p => crypto.createHash('sha256').update(fs.readFileSync(p)).digest('hex');
const receipt = {binary_sha256: sha(frozen), package: packageDir, captures: [], scope: 'Native deterministic GPU images; visual review separate'};
const cases = ['neon','forest','candy','star-sea'].flatMap(world => [6.3,26.3,46.3].map(time => ({name:`${world}-${time}`, world, time, preset:'high', width:1280, height:800})));
cases.push({name:'neon-bloom-off', world:'neon', time:6.3, preset:'no-bloom', width:1280,height:800});
cases.push({name:'neon-low-small', world:'neon', time:6.3, preset:'low', width:400,height:300});
const write = () => fs.writeFileSync(path.join(destination,'receipt.json'),JSON.stringify(receipt,null,2));
(async () => {
  for (const item of cases) {
    const png = path.join(destination,`${item.name}.png`);
    const args = ['--package',packageDir,'--presentation-smoke',item.world,String(Math.round(item.time*48000)),'timeline',item.preset,String(item.width),String(item.height),'1',png];
    const log = fs.openSync(path.join(destination,`${item.name}.log`),'wx');
    const child = spawn(frozen,args,{stdio:['ignore',log,log],detached:true});
    const timer = setTimeout(()=>{if(child.pid)try{process.kill(-child.pid,'SIGKILL');}catch(error){if(error.code!=='ESRCH')throw error;}},90000);
    let code;
    try {
      code = await new Promise((resolve,reject)=>{child.on('error',reject);child.on('exit',resolve);});
    } finally {
      clearTimeout(timer);fs.closeSync(log);
    }
    const row = {...item,args,pid:child.pid,exit:code};
    if(fs.existsSync(png)){const bytes=fs.readFileSync(png);row.size=[bytes.readUInt32BE(16),bytes.readUInt32BE(20)];row.sha256=sha(png);}
    row.pass=code===0 && row.size?.[0]===item.width && row.size?.[1]===item.height;
    receipt.captures.push(row);write();console.log(JSON.stringify(row));
    if(!row.pass)throw Error(`capture failed: ${item.name}`);
  }
  receipt.status='PASS_RENDER';write();
})().catch(error=>{receipt.status='FAIL_RENDER';receipt.error=String(error);write();console.error(error);process.exitCode=1;});
