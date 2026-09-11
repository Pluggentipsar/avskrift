// Synthetic IPC failures/progress only; no real projects or model inference.
const {chromium}=require(process.env.AVSKRIFT_PLAYWRIGHT||'playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const mocks=fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
const base=fs.readFileSync('model-tools/ui-step2.cjs','utf8');
let setup=base.slice(base.indexOf('function setup()'),base.indexOf('\n(async()=>'));
setup=setup.replace("case 'create_grounded_draft':f.lastSources",`case 'summarize':case 'ask_transcript': {
        if(f.ai)throw Error('Duplicate AI request');
        return await new Promise((resolve,reject)=>{f.ai={cmd,resolve,reject};});
      }
      case 'create_grounded_draft':if(f.holdGrounded){return await new Promise((resolve,reject)=>{f.ai={cmd,resolve,reject};});}f.lastSources`);
(async()=>{
  const browser=await chromium.launch({headless:true,channel:'msedge'});
  const page=await browser.newPage({viewport:{width:1440,height:960}}),errors=[];
  page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(mocks+'\n'+setup+'\nsetup();');
  const pending=()=>page.waitForFunction(()=>!!fixture.ai);
  const progress=message=>page.evaluate(payload=>window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'avskrift:progress',payload}),message);
  const finish=(value,fail=false)=>page.evaluate(({value,fail})=>{const a=fixture.ai;fixture.ai=null;fail?a.reject(value):a.resolve(value);},{value,fail});
  const library=()=>page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Alla projekt',exact:true}).click();
  const saved=()=>page.locator('.save-status').filter({hasText:'Sparat på datorn'}).waitFor();
  fs.mkdirSync('docs/ui-step5',{recursive:true});
  try {
    await page.goto('http://127.0.0.1:1420');await page.getByRole('heading',{name:'Ditt arbete',exact:true}).waitFor();
    await page.evaluate(()=>fixture.jobs.push(
      {version:2,id:'document',jobType:'summarize',title:'Lång sammanfattning',createdAt:'2026-09-11',updatedAt:'2026-09-11',sourceMode:'paste',sourceText:'Ett långt underlag. '.repeat(300),summaryDraft:'Tidigare kontrollerat utkast.'},
      {version:2,id:'meeting',jobType:'meeting',title:'Frågor om mötet',createdAt:'2026-09-11',updatedAt:'2026-09-11',transcript:{language:'sv',model:'small',diarized:false,utterances:[{start:0,end:5,text:'Vi köper två böcker.',speaker:null},{start:5,end:10,text:'Åsa beställer på fredag.',speaker:null}]},summaryDraft:'Befintligt mötesutkast.'}
    ));
    await library();await page.getByRole('button',{name:/Lång sammanfattning/}).click();
    await page.getByRole('button',{name:'Generera om',exact:true}).click();await pending();
    assert.equal(await page.getByRole('button',{name:'Generera om',exact:true}).isDisabled(),true);
    await progress('Bearbetar del 3 av 12…');await page.getByText('Bearbetar del 3 av 12…',{exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>fixture.jobs.find(j=>j.id==='document').summaryDraft),'Tidigare kontrollerat utkast.');
    await progress('Sammanställer, omgång 2, del 1 av 3…');await page.getByText('Sammanställer, omgång 2, del 1 av 3…',{exact:true}).waitFor();
    await page.screenshot({path:'docs/ui-step5/progress.png',fullPage:true});
    await finish('Modellens svar blev för långt och avbröts. Tidigare text finns kvar.',true);
    const draft=page.getByLabel('Sammanfattning – redigerbart utkast');await draft.waitFor();
    assert.equal(await draft.inputValue(),'Tidigare kontrollerat utkast.');
    await page.getByText('Modellens svar blev för långt och avbröts. Tidigare text finns kvar.',{exact:true}).waitFor();
    await page.getByRole('button',{name:'Generera om',exact:true}).click();await pending();
    await finish('Nytt färdigt utkast med Åsa och Östen.');await saved();
    assert.equal(await draft.inputValue(),'Nytt färdigt utkast med Åsa och Östen.');
    await library();await page.getByRole('button',{name:/Frågor om mötet/}).click();
    const grounded=page.getByRole('region',{name:'Källbelagt utkast'});
    await grounded.getByRole('button',{name:'Skapa källutkast',exact:true}).click();await grounded.getByLabel('Förslag 1',{exact:true}).waitFor();await saved();
    const original=await grounded.getByLabel('Förslag 1',{exact:true}).inputValue();
    await page.evaluate(()=>fixture.holdGrounded=true);
    await grounded.getByRole('button',{name:'Skapa nytt källutkast',exact:true}).click();await pending();
    await progress('Skapar källutkast, del 2 av 7…');await page.getByText('Skapar källutkast, del 2 av 7…',{exact:true}).waitFor();
    await finish('Mallen är för lång för modellens arbetsfönster.',true);
    await grounded.getByRole('alert').filter({hasText:'Mallen är för lång'}).waitFor();
    assert.equal(await grounded.getByLabel('Förslag 1',{exact:true}).inputValue(),original);
    await page.getByRole('button',{name:'Fråga källan',exact:true}).click();
    await page.getByPlaceholder('Fråga mötet…').fill('Vem beställer böckerna?');await page.getByRole('button',{name:'Fråga',exact:true}).click();await pending();
    await progress('Söker svar, del 4 av 8…');await page.getByText('Söker svar, del 4 av 8…',{exact:true}).waitFor();
    await page.getByRole('button',{name:'Sammanfattning',exact:true}).click();
    assert.equal(await grounded.getByRole('button',{name:'Skapa nytt källutkast',exact:true}).isDisabled(),true);
    await library();await page.getByRole('button',{name:/Lång sammanfattning/}).click();
    await page.getByRole('alert').filter({hasText:'Avsluta det pågående arbetet'}).waitFor();
    await page.getByRole('navigation',{name:'Huvudnavigation'}).getByRole('button',{name:'Möten',exact:true}).click();
    await page.getByRole('button',{name:'Fortsätt med aktuellt transkript',exact:true}).click();await page.getByRole('button',{name:'Fråga källan',exact:true}).click();
    await finish('Detta svar blev avbrutet.',true);await page.getByText('Detta svar blev avbrutet.',{exact:true}).waitFor();
    assert.equal(await page.locator('.qa-item').count(),0);
    assert.equal(await page.getByPlaceholder('Fråga mötet…').inputValue(),'Vem beställer böckerna?');
    await page.getByRole('button',{name:'Fråga',exact:true}).click();await pending();await finish('Åsa beställer på fredag.');await page.locator('.qa-a').filter({hasText:'Åsa beställer på fredag.'}).waitFor();
    await page.getByRole('button',{name:'Anteckningar och åtgärder',exact:true}).click();
    await page.getByRole('button',{name:'Föreslå åtgärder',exact:true}).click();await pending();
    await progress('Söker svar, del 2 av 5…');await page.getByText('Söker svar, del 2 av 5…',{exact:true}).waitFor();
    await finish('Det framgår inte av underlaget.');await page.getByText('Inga åtgärder hittades',{exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>fixture.jobs.find(j=>j.id==='meeting').actions?.length??0),0);
    assert.deepEqual(errors,[]);
    console.log('PASS: visible multi-stage progress, no duplicate AI request, completed-only replacement, retained summary/cited draft/question on failure, retry and project-switch protection.');
  }catch(e){console.error(errors);await page.screenshot({path:'docs/ui-step5/failure.png',fullPage:true});throw e;}finally{await browser.close();}
})();
