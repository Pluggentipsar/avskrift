// Visual audit with synthetic meeting data; no real microphone or saved user content.
const {chromium}=require(process.env.AVSKRIFT_PLAYWRIGHT||'playwright');
const fs=require('node:fs');
const mocks=fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
const base=fs.readFileSync('model-tools/ui-step2.cjs','utf8');
let setup=base.slice(base.indexOf('function setup()'),base.indexOf('\n(async()=>')).replace("id:'small',label:'Small',downloaded:true", "id:'kb-whisper-small',label:'KB Small',downloaded:true");
setup=setup.replace("case 'begin_work':", "case 'start_meeting':return; case 'stop_meeting':return {micWavPath:'synthetic-mic.wav',systemWavPath:'synthetic-system.wav',durationS:45}; case 'begin_work':");
(async()=>{
 const browser=await chromium.launch({headless:true,channel:'msedge'});
 const page=await browser.newPage({viewport:{width:1440,height:960}});
 const errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(mocks+'\n'+setup+'\nsetup();');
 const dir='docs/meeting-flow-audit';fs.mkdirSync(dir,{recursive:true});
 const shot=async name=>{await page.waitForTimeout(400);return page.screenshot({path:`${dir}/${name}.png`,fullPage:true});};
 const nav=()=>page.getByRole('navigation',{name:'Huvudnavigation'});
 try{
  await page.goto('http://127.0.0.1:1420');
  await page.getByRole('heading',{name:'Ditt arbete',exact:true}).waitFor();
  await page.evaluate(()=>fixture.jobs.push({version:2,id:'audit-meeting',jobType:'meeting',title:'Planeringsmöte – höstens aktiviteter',category:'Arbetslaget',createdAt:'2026-09-11',updatedAt:'2026-09-11',notes:'Vi behöver samordna höstens aktiviteter och boka lokal.',participants:[{name:'Alex',role:'Samordnare'}],actions:[{text:'Boka lokalen',done:false,assignee:'Alex',due:'2026-09-18'}],transcript:{language:'sv',model:'small',diarized:true,utterances:[{start:0,end:5,text:'Vi behöver boka en lokal till nästa vecka.',speaker:'Jag',words:[]},{start:5,end:10,text:'Jag tar kontakt med receptionen på fredag.',speaker:'Mötet',words:[]}]},speakerLabels:{Jag:'Jag','Mötet':'Alex'}}));
  await page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Alla projekt',exact:true}).click();
  await page.getByRole('button',{name:/Planeringsmöte – höstens aktiviteter/}).click();
  await page.getByRole('button',{name:'Transkript',exact:true}).click();await shot('transcript');
  await page.getByRole('button',{name:'Anteckningar och åtgärder',exact:true}).click();await shot('notes');
  await page.setViewportSize({width:900,height:700});await shot('notes-narrow');
  await page.setViewportSize({width:1440,height:960});
  await nav().getByRole('button',{name:'Ditt arbete',exact:true}).click();await shot('home');
  await nav().getByRole('button',{name:'Möten',exact:true}).click();await shot('setup');
  await page.getByRole('button',{name:'Starta inspelning',exact:true}).click();
  await page.getByRole('button',{name:'Anteckningar',exact:true}).click();
  await page.locator('.m-notes').fill('En anteckning under det simulerade mötet.');
  await page.evaluate(()=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'avskrift:meeting-utterance',payload:{source:'Mötet',start:0,end:4,text:'Vi börjar med dagens agenda.'}}));
  await shot('recording');
  const savedDuringRecording=await page.evaluate(()=>fixture.jobs.some(j=>j.notes==='En anteckning under det simulerade mötet.'));
  await page.getByRole('button',{name:'Stoppa & transkribera',exact:true}).click();
  await page.getByRole('button',{name:'Starta inspelning',exact:true}).waitFor();await shot('after-stop');
  const pending=await page.evaluate(()=>fixture.jobs.find(j=>j.transcriptionPending));
  await page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Alla projekt',exact:true}).click();
  await page.getByRole('button',{name:new RegExp(pending.title)}).click();
  await page.getByRole('button',{name:'Anteckningar och åtgärder',exact:true}).click();await shot('pending-notes');
  const pendingNotesVisible=await page.locator('textarea.ws-notes').count();
  const result={savedDuringRecording,pendingNotesVisible,notesSavedAtStop:pending.notes,errors};
  fs.writeFileSync(`${dir}/observations.json`,JSON.stringify(result,null,2));console.log(result);
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
