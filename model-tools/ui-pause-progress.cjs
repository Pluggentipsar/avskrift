// Pausing a meeting recording, and a running transcription with progress, time left, pause and resume.
// Mocked IPC with synthetic data.
const {chromium}=require(process.env.AVSKRIFT_PLAYWRIGHT||'playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const mocks=fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
const base=fs.readFileSync('model-tools/ui-step2.cjs','utf8');
let setup=base.slice(base.indexOf('function setup()'),base.indexOf('\n(async()=>'));
setup=setup.replace("id:'small',label:'Small',downloaded:true","id:'kb-whisper-small',label:'KB Small',downloaded:true");
setup=setup.replace("case 'begin_work':", `
 case 'meeting_devices':return [{id:'headset',name:'Testheadset',source:'mic'},{id:'speakers',name:'Testutgång',source:'system'}];
 case 'meeting_levels':case 'test_meeting_audio':return [{name:'Testheadset',peak:0.2,active:true},{name:'Testutgång',peak:0.2,active:true}];
 case 'start_meeting':{const job={...args.args.job,micWavPath:'test-mic.wav',audioPath:'test-system.wav'};f.jobs.push(job);persist();return {micWavPath:job.micWavPath,systemWavPath:job.audioPath};}
 case 'stop_meeting':return {};
 case 'pause_meeting':(f.pauses??=[]).push(args.paused);return;
 case 'pause_work':(f.workPauses??=[]).push(args.paused);return true;
 case 'cancel_work':f.cancelled=true;window.__finishTranscribe?.(Promise.reject('Arbetet avbröts. Tidigare resultat finns kvar.'));return true;
 case 'plugin:dialog|open':return 'C:/synthetic/intervju.wav';
 case 'transcribe':return new Promise((resolve,reject)=>{window.__finishTranscribe=p=>p.then(resolve,reject);});
 case 'begin_work':`);
const emit=(page,event,payload)=>page.evaluate(([event,payload])=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event,payload}),[event,payload]);
(async()=>{
 const browser=await chromium.launch({headless:true,channel:'msedge'});
 const page=await browser.newPage({viewport:{width:1440,height:900}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(mocks+'\n'+setup+'\nsetup();');
 const nav=()=>page.getByRole('navigation',{name:'Huvudnavigation'});
 const step=async(name,fn)=>{await fn();console.log(`ok  ${name}`);};
 try{
  await page.goto(process.env.AVSKRIFT_UI_URL||'http://localhost:1420');
  await step('a meeting can be paused and resumed without ending it',async()=>{
   await nav().getByRole('button',{name:'Möten',exact:true}).click();
   await page.getByLabel('Mötesnamn',{exact:true}).fill('Långt möte');
   await page.getByLabel('Välj mikrofon').selectOption('headset');
   await page.getByRole('button',{name:'Starta inspelning',exact:true}).click();
   await page.getByText(/Spelar in · 0:0[1-3]/).waitFor();
   await page.getByRole('button',{name:'Pausa inspelningen',exact:true}).click();
   await page.getByText(/Pausad · 0:0\d/).waitFor();
   const pausedAt=await page.locator('.big-rec').innerText();
   assert.equal(await page.getByText('Pausad – inget spelas in').count(),2,'both channels say nothing is recorded');
   await page.waitForTimeout(2200);
   assert.equal(await page.locator('.big-rec').innerText(),pausedAt,'the timer stands still while paused');
   await page.getByRole('button',{name:'Fortsätt inspelningen',exact:true}).click();
   await page.getByText(/Spelar in/).waitFor();
   assert.deepEqual(await page.evaluate(()=>fixture.pauses),[true,false]);
   await page.waitForFunction(()=>(fixture.jobs.at(-1).bookmarks??[]).map(b=>b.text).join('|')==='Paus|Fortsätter efter paus');
   await page.getByRole('button',{name:'Stoppa inspelningen',exact:true}).click();
  });
  await step('a transcription shows progress with time left, can be paused and resumed',async()=>{
   await nav().getByRole('button',{name:'Transkribera',exact:true}).click();
   await page.getByRole('button',{name:'Välj ljudfil…'}).click();
   await page.getByRole('button',{name:'Transkribera',exact:true}).last().click();
   await emit(page,'avskrift:progress','Transkriberar – GPU…');
   await emit(page,'avskrift:percent',0);
   await page.getByRole('button',{name:'Pausa',exact:true}).waitFor();
   await page.waitForTimeout(5500);
   await emit(page,'avskrift:percent',25);
   await page.getByText(/25 % · (ca \d+ min kvar|under en minut kvar)/).waitFor();
   await page.getByRole('button',{name:'Pausa',exact:true}).click();
   await page.getByText('25 % · pausat').waitFor();
   await page.getByText(/Pausat: transkriberar ljudfil/).waitFor();
   await page.getByRole('button',{name:'Fortsätt',exact:true}).click();
   await page.getByRole('button',{name:'Pausa',exact:true}).waitFor();
   assert.deepEqual(await page.evaluate(()=>fixture.workPauses),[true,false]);
   await page.getByRole('button',{name:'Avbryt arbete'}).click();
   await page.getByRole('button',{name:'Pausa',exact:true}).waitFor({state:'detached'});
  });
  assert.deepEqual(errors,[]);
  console.log('UI PAUSE PROGRESS: all steps passed');
 }catch(e){
  fs.mkdirSync('.build-tools',{recursive:true});await page.screenshot({path:'.build-tools/pause-progress-failure.png',fullPage:true});
  console.error(e);console.error('page errors:',errors);process.exitCode=1;
 }finally{await browser.close();}
})();
