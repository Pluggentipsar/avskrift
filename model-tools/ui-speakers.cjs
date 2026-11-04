// Telling meeting participants apart: count from the participant list, separation in the background
// after the text is done, naming from the list, listening, merging and separating again. Mocked IPC
// with synthetic data.
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
 case 'stop_meeting':f.stopArgs=args.args;return {};
 case 'diarize_meeting':(f.diarized??=[]).push(args.args.numSpeakers);return {language:'sv',model:'kb-whisper-small',diarized:true,utterances:[
   {start:1,end:3,speaker:'Jag',text:'Välkomna allihop.',words:[]},{start:4,end:9,speaker:'TALARE_1',text:'Tack, jag börjar med budgeten.',words:[]},
   {start:10,end:14,speaker:'TALARE_2',text:'Den ser bra ut.',words:[]},{start:15,end:18,speaker:'TALARE_3',text:'Jag håller med.',words:[]}]};
 case 'begin_work':`);
setup=setup.replace('const j=structuredClone(args.job);j.originalSourceText', `const j={...old,...structuredClone(args.job)};
 if(old && !old.transcriptionPending && j.transcriptionPending && old.transcript){j.transcript=old.transcript;j.transcriptionPending=false;j.mixWavPath=old.mixWavPath;}
 j.originalSourceText`);
const emit=(page,event,payload)=>page.evaluate(([event,payload])=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event,payload}),[event,payload]);
const t=(...u)=>({language:'sv',model:'kb-whisper-small',diarized:true,utterances:u.map(([start,end,speaker,text])=>({start,end,speaker,text,words:[]}))});
const plain=t([1,3,'Jag','Välkomna allihop.'],[4,9,'Mötet','Tack, jag börjar med budgeten.'],[10,14,'Mötet','Den ser bra ut.'],[15,18,'Mötet','Och resten håller.']);
const split=t([1,3,'Jag','Välkomna allihop.'],[4,9,'TALARE_1','Tack, jag börjar med budgeten.'],[10,14,'TALARE_2','Den ser bra ut.'],[15,18,'TALARE_1','Och resten håller.']);
(async()=>{
 const browser=await chromium.launch({headless:true,channel:'msedge'});
 const page=await browser.newPage({viewport:{width:1440,height:900}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(mocks+'\n'+setup+'\nsetup();');
 const nav=()=>page.getByRole('navigation',{name:'Huvudnavigation'});
 const panel=()=>page.locator('details.speaker-panel');
 const speakersInText=()=>page.locator('input.speaker').evaluateAll(els=>els.map(e=>e.value));
 const step=async(name,fn)=>{await fn();console.log(`ok  ${name}`);};
 try{
  await page.goto(process.env.AVSKRIFT_UI_URL||'http://localhost:1420');
  let id;
  await step('the participant list gives the number of voices to look for',async()=>{
   await nav().getByRole('button',{name:'Möten',exact:true}).click();
   await page.getByLabel('Mötesnamn',{exact:true}).fill('Budgetmöte');
   await page.getByText('Deltagare och sammanfattningsmall').click();
   await page.getByLabel('Deltagare inför mötet').fill('Kim; Ekonom\nAlex');
   assert.ok(await page.getByLabel('Skilj på deltagarna när mötet är klart').isChecked());
   await page.getByLabel('Antal röster att leta efter').locator('option',{hasText:'Enligt deltagarlistan (2)'}).waitFor({state:'attached'});
   await page.getByLabel('Välj mikrofon').selectOption('headset');
   await page.getByRole('button',{name:'Starta inspelning',exact:true}).click();
   await page.getByRole('button',{name:'Stoppa inspelningen',exact:true}).click();
   const stop=await page.evaluate(()=>fixture.stopArgs);
   assert.equal(stop.separateVoices,true);assert.equal(stop.numSpeakers,2);
   id=await page.evaluate(()=>fixture.jobs[0].id);
  });
  await step('the text comes first; the voices are told apart in the background',async()=>{
   await page.evaluate(([id,tr])=>{const j=fixture.jobs.find(j=>j.id===id);j.transcript=tr;j.transcriptionPending=false;},[id,plain]);
   await emit(page,'avskrift:meeting-done',{token:id,transcript:plain,mixWavPath:'test-mix.wav',separatingVoices:true});
   await page.getByRole('navigation',{name:'Vyer i aktuellt arbete'}).getByRole('button',{name:'Transkript',exact:true}).click();
   await page.getByText('Och resten håller.').waitFor();
   await page.getByRole('button',{name:'Redigera',exact:true}).waitFor();
   await panel().getByText(/Skiljer på deltagarna i bakgrunden/).waitFor();
   assert.ok(await panel().getByRole('button',{name:'Skilj på deltagarna'}).isDisabled());
  });
  await step('when it is done the speakers show up, ready to be named',async()=>{
   await emit(page,'avskrift:meeting-voices',{token:id,status:'done',transcript:split,speakerLabels:{TALARE_1:'Talare 1',TALARE_2:'Talare 2'}});
   await page.getByText(/2 röster hittades i mötet/).waitFor();
   assert.deepEqual(await speakersInText(),['Jag','Talare 1','Talare 2','Talare 1']);
   assert.equal(await panel().getByText(/Skiljer på deltagarna i bakgrunden/).count(),0);
   assert.equal(await panel().locator('li').count(),3);
  });
  await step('a speaker is named from the participant list everywhere at once',async()=>{
   const names=await page.locator('#speaker-names option').evaluateAll(o=>o.map(x=>x.value));
   assert.deepEqual(names,['Kim','Alex']);
   const input=panel().getByLabel('Namn på Talare 1');
   await input.fill('Kim');await input.press('Enter');
   await page.waitForFunction(()=>[...document.querySelectorAll('input.speaker')].filter(e=>e.value==='Kim').length===2);
   await page.waitForFunction(()=>fixture.jobs[0].speakerLabels?.TALARE_1==='Kim');
  });
  await step('listening plays a short stretch of that speaker',async()=>{
   await panel().getByRole('button',{name:'Lyssna på Kim'}).click();
   await page.waitForTimeout(300);
  });
  await step('two voices that are one person can be merged, and undone',async()=>{
   await panel().getByLabel('Slå ihop Talare 2 med').selectOption({label:'Kim'});
   await page.getByText('Talare 2 slogs ihop med Kim').waitFor();
   assert.deepEqual(await speakersInText(),['Jag','Kim'],'one heading for consecutive lines');
   await page.waitForFunction(()=>fixture.jobs[0].transcript.utterances.filter(u=>u.speaker==='TALARE_1').length===3);
   await page.getByRole('button',{name:'Ångra',exact:true}).click();
   assert.deepEqual(await speakersInText(),['Jag','Kim','Talare 2','Kim']);
  });
  await step('separating again with another count starts the names over',async()=>{
   await panel().getByLabel('Antal röster i mötet').selectOption('3');
   await panel().getByRole('button',{name:'Skilj på deltagarna igen'}).click();
   await page.getByText(/3 röster hittades i mötet/).waitFor();
   assert.deepEqual(await page.evaluate(()=>fixture.diarized),[3]);
   assert.deepEqual(await speakersInText(),['Jag','Talare 1','Talare 2','Talare 3']);
   if(process.env.AVSKRIFT_SHOTS)await page.screenshot({path:'.build-tools/speakers-panel.png'});
  });
  await step('a background result never overwrites text you edited meanwhile',async()=>{
   await emit(page,'avskrift:meeting-voices',{token:id,status:'done',transcript:split,speakerLabels:{}});
   await page.getByText(/Texten ändrades innan deltagarna hann skiljas åt/).waitFor();
   assert.deepEqual(await speakersInText(),['Jag','Talare 1','Talare 2','Talare 3']);
  });
  await step('the overview has a button that leads to the speakers',async()=>{
   await panel().locator('summary').click();
   assert.equal(await panel().getAttribute('open'),null);
   await page.getByRole('navigation',{name:'Vyer i aktuellt arbete'}).getByRole('button',{name:'Översikt',exact:true}).click();
   await page.getByTitle('Dela upp mötesljudet i de som pratade och ge dem namn').click();
   assert.equal(await panel().getAttribute('open'),'');
  });
  assert.deepEqual(errors,[]);
  console.log('UI SPEAKERS: all steps passed');
 }catch(e){
  fs.mkdirSync('.build-tools',{recursive:true});await page.screenshot({path:'.build-tools/speakers-failure.png',fullPage:true});
  console.error(e);console.error('page errors:',errors);process.exitCode=1;
 }finally{await browser.close();}
})();
