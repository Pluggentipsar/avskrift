// Synthetic fixtures only. No native app, microphone, real projects or model downloads.
const {chromium}=require(process.env.AVSKRIFT_PLAYWRIGHT||'playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const mocks=fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
const base=fs.readFileSync('model-tools/ui-step2.cjs','utf8');
let setup=base.slice(base.indexOf('function setup()'),base.indexOf('\n(async()=>'))
  .replace("const persist=()=>sessionStorage.setItem('synthetic-store',JSON.stringify(f));","const persist=()=>{};")
  .replace("case 'list_whisper_models':return [{id:'small',label:'Small',downloaded:true}];","case 'list_whisper_models':return [{id:'small',label:'Small',sizeMb:480,downloaded:true},{id:'tiny',label:'Tiny',sizeMb:80,downloaded:!!f.downloaded}];")
  .replace("case 'list_summary_models':return [{id:'qwen2.5-3b',label:'Qwen',downloaded:true}];","case 'list_summary_models':return [{id:'qwen2.5-3b',label:'Qwen',downloaded:true},{id:'text2',label:'Textmodell 2',sizeMb:900,downloaded:true}];")
  .replace("case 'dictation_snapshot':",`case 'download_whisper_model':if(f.failDownload)throw Error('Test: hämtningen misslyckades');f.downloaded=true;return;
      case 'configure_dictation':if(f.failConfigure)throw Error('Test: modellvalet misslyckades');f.dictation.settings=args.settings;f.dictation.revision++;return;
      case 'dictation_snapshot':`);
(async()=>{
  const browser=await chromium.launch({headless:true,channel:'msedge'});
  const page=await browser.newPage({viewport:{width:1440,height:960}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(mocks+'\n'+setup+'\nsetup();');
  fs.mkdirSync('docs/ui-step4',{recursive:true});
  const models=page.getByRole('dialog',{name:'Modeller på datorn'});
  const openModels=()=>page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Modeller på datorn'}).click();
  const row=i=>page.locator(`[data-segment="${i}"]`);
  const inViewport=async i=>page.waitForFunction(index=>{
    const e=document.querySelector(`[data-segment="${index}"]`),r=document.querySelector('.transcript');if(!e||!r)return false;
    const a=e.getBoundingClientRect(),b=r.getBoundingClientRect();return a.top>=b.top-1&&a.top<b.bottom&&a.bottom>b.top;
  },i,{timeout:8000});
  try{
    await page.goto('http://127.0.0.1:1420');
    await page.getByRole('heading',{name:'Ditt arbete',exact:true}).waitFor();
    await openModels();await models.getByLabel('Talmodell för möten').selectOption('small');
    await models.getByLabel('Talmodell för diktering').selectOption('tiny');
    await page.waitForFunction(()=>fixture.dictation.settings.model==='tiny');
    assert.equal(await models.getByLabel('Talmodell för möten').inputValue(),'small');
    await page.evaluate(()=>fixture.failDownload=true);
    await models.getByRole('button',{name:'Hämta modell',exact:true}).click();
    await models.getByRole('alert').filter({hasText:'Test: hämtningen misslyckades'}).waitFor();
    await page.evaluate(()=>fixture.failDownload=false);
    await models.getByRole('button',{name:'Hämta modell',exact:true}).click();
    await models.getByRole('button',{name:'Hämta modell',exact:true}).waitFor({state:'hidden'});
    await page.evaluate(()=>fixture.failConfigure=true);
    await models.getByLabel('Talmodell för diktering').selectOption('small');
    await models.getByRole('alert').filter({hasText:'Test: modellvalet misslyckades'}).waitFor();
    assert.equal(await models.getByLabel('Talmodell för diktering').inputValue(),'tiny');
    await page.evaluate(()=>fixture.failConfigure=false);
    await models.getByLabel('Talmodell för diktering').selectOption('small');
    await page.waitForFunction(()=>fixture.dictation.settings.model==='small');
    await models.getByLabel('Textmodell',{exact:true}).selectOption('text2');
    await page.screenshot({path:'docs/ui-step4/models.png',fullPage:true});
    await page.keyboard.press('Escape');
    await page.getByRole('navigation',{name:'Huvudnavigation'}).getByRole('button',{name:'Diktering',exact:true}).click();
    await page.getByText('Textbearbetning: text2',{exact:false}).waitFor();
    await page.getByRole('button',{name:'Modeller för tal och text'}).click();
    assert.equal(await models.getByLabel('Textmodell',{exact:true}).inputValue(),'text2');
    await models.getByRole('button',{name:'Tillbaka till arbetet'}).click();
    await page.evaluate(()=>{
      const utterances=Array.from({length:2000},(_,i)=>{
        const texts=Array.from({length:25},(_,w)=>w===0?`Avsnitt${i}`:'ord');
        if(i===900||i===1900)texts[2]='Årsplanen';
        return {start:i*5,end:i*5+5,speaker:'A',text:texts.join(' '),words:texts.map((text,w)=>({text,start:i*5+w*.2,end:i*5+(w+1)*.2}))};
      });
      fixture.jobs.push({version:2,id:'long',jobType:'meeting',title:'Långt testmöte',createdAt:'2026-09-11',updatedAt:'2026-09-11',audioPath:'C:/synthetic/long.wav',transcript:{language:'sv',model:'small',diarized:true,utterances},speakerLabels:{A:'Mötesdeltagaren'}});
    });
    await page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Alla projekt',exact:true}).click();
    const started=Date.now();await page.getByRole('button',{name:/Långt testmöte/}).click();
    await page.getByRole('button',{name:'Transkript',exact:true}).click();await row(0).waitFor();
    const mounted=await page.locator('.word').count();assert.ok(mounted<2500,`Mounted ${mounted} of 50,000 words`);
    const openMs=Date.now()-started;
    // Fake media clock exercises UI playback and seeking, without native media or capture.
    await page.evaluate(()=>{
      const a=document.querySelector('audio');Object.defineProperty(a,'readyState',{get:()=>1});
      Object.defineProperty(a,'currentTime',{value:0,writable:true});Object.defineProperty(a,'duration',{get:()=>10000});
      a.play=async()=>{a.dispatchEvent(new Event('play'));a.dispatchEvent(new Event('timeupdate'));};
      a.pause=()=>a.dispatchEvent(new Event('pause'));
      window.playAt=t=>{a.currentTime=t;a.dispatchEvent(new Event('timeupdate'));a.dispatchEvent(new Event('play'));};
    });
    await page.keyboard.press('Control+f');assert.equal(await page.getByLabel('Sök i transkript',{exact:true}).evaluate(el=>document.activeElement===el),true);
    await page.getByLabel('Sök i transkript',{exact:true}).fill('ÅRSPLANEN');await inViewport(900);
    await page.getByText('1 av 2 avsnitt',{exact:true}).waitFor();
    await page.getByRole('button',{name:'Nästa träff',exact:true}).click();await inViewport(1900);
    await page.getByRole('button',{name:'Nästa träff',exact:true}).click();await inViewport(900);
    await page.getByRole('button',{name:'Föregående träff',exact:true}).click();await inViewport(1900);
    await row(1900).locator('.body').focus();await page.keyboard.press('ArrowDown');await inViewport(1901);
    assert.equal(await row(1901).locator('.body').evaluate(el=>document.activeElement===el),true);
    await page.keyboard.press('Control+Home');await inViewport(0);
    await page.keyboard.press('Control+End');await inViewport(1999);
    await page.getByLabel('Sök i transkript',{exact:true}).fill('Avsnitt1900');await inViewport(1900);
    await row(1900).locator('.body').dblclick();const editor=page.getByLabel('Redigera avsnitt',{exact:true});await editor.fill('Åsa rättade detta avsnitt.');
    // Wheel/scroll must not unmount or commit the active editor.
    await page.locator('.transcript').evaluate(el=>{el.scrollTop=0;el.dispatchEvent(new Event('scroll'));});
    await page.waitForTimeout(180);assert.equal(await editor.count(),1);assert.equal(await editor.inputValue(),'Åsa rättade detta avsnitt.');
    await editor.press('Control+Enter');await page.waitForFunction(()=>fixture.jobs.find(j=>j.id==='long').transcript.utterances[1900].text==='Åsa rättade detta avsnitt.');
    await page.getByRole('button',{name:'Ångra',exact:true}).click();await page.getByLabel('Sök i transkript',{exact:true}).fill('ÅRSPLANEN');await inViewport(900);
    await page.getByLabel('Sök i transkript',{exact:true}).fill('Avsnitt1910');await inViewport(1910);
    await row(1910).locator('.body').focus();await page.keyboard.press('Enter');await editor.fill('Detta ska inte sparas.');await editor.press('Escape');
    assert.equal(await row(1910).locator('.word').count(),25);
    await page.getByRole('button',{name:'Redigera',exact:true}).click();
    await row(1910).getByLabel('Talare för avsnitt 1911',{exact:true}).selectOption('');
    await page.waitForFunction(()=>fixture.jobs.find(j=>j.id==='long').transcript.utterances[1910].speaker===null);
    await row(1910).getByRole('button',{name:'Ta bort avsnitt 1911',exact:true}).click();
    await page.waitForFunction(()=>fixture.jobs.find(j=>j.id==='long').transcript.utterances.length===1999);
    await page.getByRole('button',{name:'Ångra',exact:true}).click();
    await page.waitForFunction(()=>fixture.jobs.find(j=>j.id==='long').transcript.utterances.length===2000);
    await page.getByRole('button',{name:'✓ Redigerar',exact:true}).click();
    await page.getByLabel('Sök i transkript',{exact:true}).fill('');
    await page.getByRole('checkbox',{name:'Följ uppspelningen'}).check();await page.evaluate(()=>playAt(8500.3));await inViewport(1700);
    assert.ok(await row(1700).locator('.word.playing').count()>0);
    await page.locator('.transcript').dispatchEvent('wheel',{deltaY:100});assert.equal(await page.getByRole('checkbox',{name:'Följ uppspelningen'}).isChecked(),false);
    await page.evaluate(()=>playAt(9000.3));await row(1700).waitFor();
    await page.getByRole('checkbox',{name:'Följ uppspelningen'}).check();await inViewport(1800);
    await row(1800).locator('.word').nth(4).click();assert.equal(await page.locator('audio').evaluate(a=>a.currentTime),9000.8);
    await page.getByLabel('Textstorlek',{exact:true}).selectOption('22');await inViewport(1800);
    await page.screenshot({path:'docs/ui-step4/transcript.png',fullPage:true});
    await page.setViewportSize({width:720,height:900});
    await page.getByLabel('Sök i transkript',{exact:true}).fill('Avsnitt1999');await inViewport(1999);await page.waitForTimeout(400);await inViewport(1999);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
    await page.screenshot({path:'docs/ui-step4/narrow.png',fullPage:true});
    await page.getByRole('button',{name:'Visa verktyg för transkriptet',exact:true}).click();
    await page.getByRole('button',{name:'Tillämpa på hela transkriptet'}).waitFor();
    await page.getByRole('button',{name:'Dölj verktyg för transkriptet',exact:true}).click();
    assert.equal(await page.locator('.sidebar').getAttribute('inert'),'');
    assert.ok(await page.locator('.word').count()<2500);
    assert.deepEqual(errors,[]);
    fs.writeFileSync('docs/ui-step4/measurements.json',JSON.stringify({syntheticWords:50000,mountedOnOpen:mounted,openMs,note:'Headless Edge, mocked IPC; not native transcription speed.'},null,2));
    console.log(`PASS: 50,000-word transcript (${mounted} mounted initially), search across virtual blocks, keyboard navigation, retained editor, save/undo, playback follow/seek, text size, narrow view and shared models.`);
  }catch(e){console.error(errors);await page.screenshot({path:'docs/ui-step4/failure.png',fullPage:true});throw e;}finally{await browser.close();}
})();
