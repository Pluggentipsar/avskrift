// Synthetic end-to-end meeting workflow. No physical audio devices or private files.
const {chromium}=require(process.env.AVSKRIFT_PLAYWRIGHT||'playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const mocks=fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
const base=fs.readFileSync('model-tools/ui-step2.cjs','utf8');
let setup=base.slice(base.indexOf('function setup()'),base.indexOf('\n(async()=>'));
setup=setup.replace("id:'small',label:'Small',downloaded:true","id:'kb-whisper-small',label:'KB Small',downloaded:true");
setup=setup.replace("case 'begin_work':", `
 case 'meeting_devices':return [{id:'headset',name:'Testheadset',source:'mic'},{id:'speakers',name:'Testutgång',source:'system'}];
 case 'meeting_levels':case 'test_meeting_audio':return [{name:'Testheadset',peak:0.004,active:true},{name:'Testutgång',peak:0.2,active:true}];
 case 'start_meeting':{const job={...args.args.job,micWavPath:'test-mic.wav',audioPath:'test-system.wav'};f.jobs.push(job);persist();return {micWavPath:job.micWavPath,systemWavPath:job.audioPath};}
 case 'stop_meeting':return {};
 case 'organize_job':{const j=f.jobs.find(j=>j.id===args.id);if(args.pinned!==undefined)j.pinned=args.pinned;if(args.archived!==undefined)j.archived=args.archived;persist();return;}
 case 'update_job_meta':{Object.assign(f.jobs.find(j=>j.id===args.id),{title:args.title,category:args.category});persist();return;}
 case 'refresh_library':return f.jobs.length;
 case 'ask_transcript':return 'Det framgår inte av underlaget.';
 case 'preview_transcript':return 'Testtranskript';
 case 'begin_work':`);
setup=setup.replace('const j=structuredClone(args.job);j.originalSourceText', `const j={...old,...structuredClone(args.job)};
 if(old && !old.transcriptionPending && j.transcriptionPending && old.transcript){j.transcript=old.transcript;j.transcriptionPending=false;j.mixWavPath=old.mixWavPath;}
 j.originalSourceText`);
(async()=>{
 const browser=await chromium.launch({headless:true,channel:'msedge'});
 const page=await browser.newPage({viewport:{width:1440,height:960}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(mocks+'\n'+setup+'\nsetup();');
 const dir='docs/ui-step9';fs.mkdirSync(dir,{recursive:true});
 const shot=async name=>{await page.waitForTimeout(400);await page.screenshot({path:`${dir}/${name}.png`,fullPage:true});};
 const nav=()=>page.getByRole('navigation',{name:'Huvudnavigation'});
 const tabs=()=>page.getByRole('navigation',{name:'Vyer i aktuellt arbete'});
 try{
  await page.goto('http://127.0.0.1:1420');await nav().getByRole('button',{name:'Möten',exact:true}).click();
  await page.getByLabel('Mötesnamn',{exact:true}).fill('Arbetslagets planering');
  await page.locator('.meeting-card textarea').first().fill('Planera höstens aktiviteter.');
  await page.getByLabel('Välj mikrofon').selectOption('headset');
  await page.getByRole('button',{name:'Testa ljud i 3 sekunder'}).click();
  await page.getByText(/Mikrofon: ljud registrerat/).waitFor();await shot('setup');
  await page.getByRole('button',{name:'Starta inspelning',exact:true}).click();
  await page.locator('.m-notes').fill('Anteckning under pågående inspelning.');
  await page.waitForFunction(()=>fixture.jobs.some(j=>j.notes==='Anteckning under pågående inspelning.'));
  const id=await page.evaluate(()=>fixture.jobs[0].id);
  await page.getByRole('button',{name:'Byt namn',exact:true}).click();await page.getByLabel('Namn på arbetet').fill('Planeringsmöte med arbetslaget');await page.getByRole('button',{name:'Spara namn'}).click();
  await page.waitForFunction(()=>fixture.jobs[0].title==='Planeringsmöte med arbetslaget');
  await page.getByRole('button',{name:'Markera här',exact:true}).click();await page.getByLabel('Tidsmarkerad anteckning').fill('Kontrollera lokalbokningen.');
  await page.waitForFunction(()=>fixture.jobs[0].bookmarks?.[0]?.text==='Kontrollera lokalbokningen.');await shot('recording');
  await page.getByRole('button',{name:'Stoppa inspelningen',exact:true}).click();await tabs().getByRole('button',{name:'Översikt',exact:true}).waitFor();
  assert.equal(await page.evaluate(()=>fixture.jobs.length),1);
  await tabs().getByRole('button',{name:'Anteckningar',exact:true}).click();
  assert.equal(await page.locator('textarea.ws-notes').inputValue(),'Anteckning under pågående inspelning.');
  await page.locator('textarea.ws-notes').fill('Redigering medan transkriptet bearbetas.');
  // Deliver a native result before the autosave timer. Both versions must survive.
  await page.evaluate(async()=>{
    const j=fixture.jobs[0];j.transcript={language:'sv',model:'kb-whisper-small',diarized:true,utterances:[{start:0,end:4,speaker:'Jag',text:'Vi bokar lokalen på fredag.',words:[]},{start:4,end:8,speaker:'Mötet',text:'Jag kontaktar receptionen.',words:[]}]};j.transcriptionPending=false;j.mixWavPath='test-mix.wav';
    await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'avskrift:meeting-done',payload:{token:j.id,transcript:j.transcript,mixWavPath:j.mixWavPath}});
  });
  await page.waitForFunction(()=>fixture.jobs[0].notes==='Redigering medan transkriptet bearbetas.'&&!fixture.jobs[0].transcriptionPending);
  assert.equal(await page.evaluate(()=>fixture.jobs[0].transcript.utterances.length),2);
  assert.equal(await page.locator('textarea.ws-notes').inputValue(),'Redigering medan transkriptet bearbetas.');await shot('notes');
  await tabs().getByRole('button',{name:'Beslut och åtgärder',exact:true}).click();
  await page.getByLabel('Nytt beslut').fill('Boka lokal till fredag.');await page.getByRole('button',{name:'Lägg till beslut',exact:true}).click();
  await page.locator('.ws-add input').fill('Kontakta receptionen');await page.locator('.ws-add button').click();
  await page.locator('.ws-assignee').fill('Alex');await page.locator('.ws-due').fill('2026-09-18');
  await page.waitForFunction(()=>fixture.jobs[0].actions?.[0]?.assignee==='Alex');await shot('actions');
  await tabs().getByRole('button',{name:'Översikt',exact:true}).click();await page.locator('.followup-date input').fill('2026-09-18');await shot('overview');
  await page.getByRole('button',{name:'Exportera mötesunderlag',exact:true}).click();
  await page.getByRole('dialog').waitFor();await page.waitForFunction(()=>document.querySelector('#export-preview')?.value.includes('Boka lokal till fredag.'));await page.getByRole('dialog').getByRole('checkbox',{name:'Beslut',exact:true}).uncheck();await page.waitForFunction(()=>!document.querySelector('#export-preview')?.value.includes('Boka lokal till fredag.'));
  await page.getByRole('dialog').getByRole('button',{name:/Stäng|Avbryt/}).click();
  await page.getByRole('button',{name:'Förbered uppföljningsmöte'}).click();assert.ok((await page.getByLabel('Mötesnamn',{exact:true}).inputValue()).startsWith('Uppföljning:'));
  await page.getByText('1 öppna åtgärder följer med från föregående möte.').waitFor();
  await nav().getByRole('button',{name:'Ditt arbete',exact:true}).click();await shot('home');
  const menu=page.locator('.work-menu').first();await menu.locator('summary').click();await menu.getByRole('button',{name:'Fäst',exact:true}).click();
  await page.waitForFunction(()=>fixture.jobs[0].pinned===true);
  await page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Bibliotek',exact:true}).click();
  await page.locator('.work-menu').first().locator('summary').click();await page.locator('.work-menu').first().getByRole('button',{name:'Arkivera',exact:true}).click();
  await page.waitForFunction(()=>fixture.jobs[0].archived===true);await page.getByLabel('Biblioteksfilter').selectOption('archived');
  await page.locator('.job-item').first().waitFor();await shot('library');
  await page.locator('.work-menu').first().locator('summary').click();await page.locator('.work-menu').first().getByRole('button',{name:'Återställ från arkivet'}).click();await page.getByLabel('Biblioteksfilter').selectOption('active');
  await page.locator('.job-item .job-row').first().click();
  assert.equal(await page.evaluate(()=>fixture.jobs[0].id),id);
  await tabs().getByRole('button',{name:'Anteckningar',exact:true}).click();await page.waitForFunction(()=>fixture.jobs[0].lastView==='notes');
  await page.reload();await page.locator('.job-strip .job-row').filter({hasText:'Planeringsmöte'}).first().click();
  assert.equal(await page.locator('textarea.ws-notes').inputValue(),'Redigering medan transkriptet bearbetas.');
  await page.setViewportSize({width:900,height:700});await shot('narrow');assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
  assert.deepEqual(errors,[]);console.log('PASS meeting identity, live autosave, rename, bookmarks, stop, late result, notes, decisions, actions, overview, export, followup, pin/archive, reopen and narrow layout.');
 }catch(e){await shot('failure');throw e;}finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
