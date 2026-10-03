// After a live-transcribed meeting stops, its live text is readable at once (read-only, never kept as
// the original) and is replaced when the completed transcript arrives. Synthetic data, mocked IPC.
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
 case 'begin_work':`);
setup=setup.replace('const j=structuredClone(args.job);j.originalSourceText', `const j={...old,...structuredClone(args.job)};
 if(old && !old.transcriptionPending && j.transcriptionPending && old.transcript){j.transcript=old.transcript;j.transcriptionPending=false;j.mixWavPath=old.mixWavPath;}
 j.originalSourceText`);
const emit=(page,event,payload)=>page.evaluate(([event,payload])=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event,payload}),[event,payload]);
(async()=>{
 const browser=await chromium.launch({headless:true,channel:'msedge'});
 const page=await browser.newPage({viewport:{width:1440,height:900}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(mocks+'\n'+setup+'\nsetup();');
 const nav=()=>page.getByRole('navigation',{name:'Huvudnavigation'});
 const tabs=()=>page.getByRole('navigation',{name:'Vyer i aktuellt arbete'});
 const step=async(name,fn)=>{await fn();console.log(`ok  ${name}`);};
 try{
  await page.goto(process.env.AVSKRIFT_UI_URL||'http://localhost:1420');
  await nav().getByRole('button',{name:'Möten',exact:true}).click();
  await page.getByLabel('Mötesnamn',{exact:true}).fill('Veckomöte');
  await page.getByLabel('Välj mikrofon').selectOption('headset');
  await page.getByRole('button',{name:'Starta inspelning',exact:true}).click();
  await page.getByRole('button',{name:'Stoppa inspelningen',exact:true}).waitFor();
  const id=await page.evaluate(()=>fixture.jobs[0].id);
  await emit(page,'avskrift:meeting-utterance',{source:'Jag',start:1,end:3,text:'Live ett från mig.'});
  await emit(page,'avskrift:meeting-utterance',{source:'Mötet',start:4,end:7,text:'Live två från mötet.'});
  await step('stop shows the live text at once, read-only, with what happens next',async()=>{
   await page.getByRole('button',{name:'Stoppa inspelningen',exact:true}).click();
   await page.getByText('Live två från mötet.').waitFor();
   assert.equal(await tabs().getByRole('button',{name:'Transkript',exact:true}).getAttribute('aria-pressed'),'true');
   await page.getByText('Preliminär text från mötet.').waitFor();
   assert.equal(await page.getByRole('button',{name:'Redigera',exact:true}).count(),0);
  });
  await step('progress and the backend\'s provisional transcript update the view',async()=>{
   await emit(page,'avskrift:meeting-progress',{token:id,msg:'Transkriberar det som saknas: mötet från 0:07…'});
   await page.getByText(/det som saknas: mötet från 0:07/).waitFor();
   await emit(page,'avskrift:meeting-provisional',{token:id,transcript:{language:'sv',model:'kb-whisper-small',diarized:true,utterances:[
     {start:1,end:3,speaker:'Jag',text:'Live ett från mig.',words:[]},{start:4,end:7,speaker:'Mötet',text:'Live två från mötet.',words:[]},{start:8,end:9,speaker:'Mötet',text:'Live tre i kön.',words:[]}]}});
   await page.getByText('Live tre i kön.').waitFor();
  });
  await step('the provisional text is never saved as the original',async()=>{
   await page.evaluate(()=>window.dispatchEvent(new Event('blur')));
   await page.waitForTimeout(1500);
   const saved=await page.evaluate(()=>fixture.jobs[0]);
   assert.ok(saved.transcriptionPending);
   assert.ok(!saved.originalTranscript?.utterances?.length,'no original while pending');
  });
  await step('the completed transcript replaces it and editing opens',async()=>{
   await page.evaluate(async()=>{
    const j=fixture.jobs[0];j.transcript={language:'sv',model:'kb-whisper-small',diarized:true,utterances:[{start:1,end:3,speaker:'Jag',text:'Live ett från mig.',words:[]},{start:4,end:7,speaker:'Mötet',text:'Live två från mötet.',words:[]},{start:8,end:12,speaker:'Mötet',text:'Slutet av mötet, transkriberat efteråt.',words:[]}]};j.transcriptionPending=false;j.mixWavPath='test-mix.wav';
    await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'avskrift:meeting-done',payload:{token:j.id,transcript:j.transcript,mixWavPath:j.mixWavPath}});
   });
   await page.getByText('Slutet av mötet, transkriberat efteråt.').waitFor();
   assert.equal(await page.getByText('Preliminär text från mötet.').count(),0);
   await page.getByRole('button',{name:'Redigera',exact:true}).waitFor();
   await tabs().getByRole('button',{name:'Anteckningar',exact:true}).click();
   await page.locator('textarea.ws-notes').fill('Efter mötet.');
   await page.waitForFunction(()=>fixture.jobs[0].notes==='Efter mötet.');
   const original=await page.evaluate(()=>fixture.jobs[0].originalTranscript?.utterances?.map(u=>u.text)??[]);
   assert.ok(!original.includes('Live tre i kön.'),'original is the completed transcript, not the provisional one');
  });
  await step('a meeting with no live text still opens on the overview',async()=>{
   await nav().getByRole('button',{name:'Möten',exact:true}).click();
   await page.getByLabel('Mötesnamn',{exact:true}).fill('Tyst möte');
   await page.getByRole('button',{name:'Starta inspelning',exact:true}).click();
   await page.getByRole('button',{name:'Stoppa inspelningen',exact:true}).click();
   await page.waitForFunction(()=>document.querySelector('nav[aria-label="Vyer i aktuellt arbete"] button[aria-pressed="true"]')?.textContent==='Översikt');
  });
  assert.deepEqual(errors,[]);
  console.log('UI MEETING PROVISIONAL: all steps passed');
 }catch(e){
  fs.mkdirSync('.build-tools',{recursive:true});await page.screenshot({path:'.build-tools/meeting-provisional-failure.png',fullPage:true});
  console.error(e);console.error('page errors:',errors);process.exitCode=1;
 }finally{await browser.close();}
})();
