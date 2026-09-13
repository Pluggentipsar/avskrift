// Synthetic browser/IPC regression. Does not claim real model or physical audio coverage.
const {chromium}=require(process.env.AVSKRIFT_PLAYWRIGHT||'playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const mocks=fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
const base=fs.readFileSync('model-tools/ui-step2.cjs','utf8');
let setup=base.slice(base.indexOf('function setup()'),base.indexOf('\n(async()=>'));
setup=setup.replace("case 'begin_work':",`
 case 'organize_job':return;
 case 'list_document_templates':return structuredClone(f.bank??=[window.demoTemplate]);
 case 'save_document_template':{const bank=f.bank??=[window.demoTemplate];const old=bank.find(t=>t.id===args.template.id);assertRevision=old?.revision??null;if(assertRevision!==args.expected)throw Error('Konflikt');const t={...args.template,revision:(old?.revision??0)+1};f.bank=[...bank.filter(v=>v.id!==t.id),t];return structuredClone(t);}
 case 'import_document_template':return {...window.demoTemplate,name:'Importerad supportmall'};
 case 'export_document_template':f.exportedTemplate=structuredClone(args.template);return;
 case 'plugin:dialog|open':return 'synthetic-template.json';
 case 'plugin:dialog|save':return 'synthetic-export.md';
 case 'template_package':f.package=structuredClone(args);return 'Instruktion: Gissa inte.\\n'+JSON.stringify(args);
 case 'create_template_draft':{if(f.failGeneration)throw Error('Test: modellen kunde inte köras');return structuredClone(window.demoValues);}
 case 'save_summary':f.exportedText=args.args.text;return;
 case 'begin_work':`);
setup=setup.replace('assertRevision=old?.revision??null;','const assertRevision=old?.revision??null;');
(async()=>{
 const browser=await chromium.launch({headless:true,channel:'msedge'});
 const page=await browser.newPage({viewport:{width:1440,height:1050}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(`window.demoTemplate=${fs.readFileSync('src/lib/support-template.json','utf8')};window.demoValues=${JSON.stringify(JSON.parse(fs.readFileSync('src/lib/support-demo.json','utf8')).values)};`+mocks+'\n'+setup+'\nsetup();Object.defineProperty(navigator,"clipboard",{value:{writeText:async text=>{window.fixture.clipboard=text}}});');
 const nav=()=>page.getByRole('navigation',{name:'Bibliotek och verktyg'});
 const workspace=()=>page.getByRole('region',{name:'Skapa från mall',exact:true});
 const draftFields=()=>workspace().locator('.fields');
 try{
  await page.goto('http://127.0.0.1:1420');await nav().getByRole('button',{name:'Sammanfatta text',exact:true}).click();
  await page.getByRole('button',{name:'Öppna fiktivt supportdemo med förberett utkast'}).first().click();
  await workspace().getByRole('heading',{name:'Skapa från mall',exact:true}).waitFor();
  fs.mkdirSync('docs/ui-templates',{recursive:true});await page.screenshot({path:'docs/ui-templates/support-demo.png',fullPage:true});
  assert.equal(await draftFields().locator('textarea').count(),8);
  const original=await page.evaluate(()=>JSON.stringify(fixture.jobs[0].transcript));
  await draftFields().getByLabel('Plats och utrustning',{exact:true}).fill('Plan två. Manuellt kompletterat id: DEMO-17.');
  await page.getByRole('navigation',{name:'Huvudnavigation'}).getByRole('button',{name:'Ditt arbete',exact:true}).click();
  await page.getByText('FIKTIVT DEMO – skrivaren på plan två',{exact:true}).first().click();
  await workspace().waitFor();assert.match(await draftFields().getByLabel('Plats och utrustning',{exact:true}).inputValue(),/DEMO-17/);
  await page.evaluate(()=>fixture.failGeneration=true);await workspace().getByRole('button',{name:'Bearbeta lokalt',exact:true}).click();await workspace().getByRole('alert').filter({hasText:'modellen kunde inte köras'}).waitFor();
  assert.match(await draftFields().getByLabel('Plats och utrustning',{exact:true}).inputValue(),/DEMO-17/);
  await page.evaluate(()=>fixture.failGeneration=false);await workspace().getByRole('button',{name:'Bearbeta lokalt',exact:true}).click();await workspace().getByText('Lokalt AI-utkast',{exact:false}).waitFor();assert.equal(await workspace().getByLabel('Sparade utkast').locator('option').count(),2);
  await workspace().getByRole('button',{name:'Duplicera mall',exact:true}).click();await workspace().getByLabel('Mallnamn',{exact:true}).fill('Support med egen rubrik');await workspace().getByRole('button',{name:'Spara mall',exact:true}).click();await workspace().getByText('Mallen är sparad på datorn.',{exact:true}).waitFor();
  await workspace().getByRole('button',{name:'Redigera mall',exact:true}).click();await workspace().getByLabel('Mallnamn',{exact:true}).fill('Support version två');await workspace().getByRole('button',{name:'Spara mall',exact:true}).click();await page.waitForFunction(()=>fixture.bank.some(t=>t.name==='Support version två'&&t.revision===2));
  // Existing drafts retain their built-in template snapshot.
  assert.match(await workspace().locator('.draft>.hint').textContent(),/Mallversion 1/);
  await workspace().getByRole('button',{name:'Exportera mall',exact:true}).click();await page.waitForFunction(()=>fixture.exportedTemplate?.revision===2);
  await workspace().getByRole('button',{name:'Importera mall',exact:true}).click();await workspace().getByLabel('Mallnamn',{exact:true}).waitFor();assert.equal(await workspace().getByLabel('Mallnamn',{exact:true}).inputValue(),'Importerad supportmall');await workspace().getByRole('button',{name:'Spara mall',exact:true}).click();
  await workspace().getByRole('button',{name:'Kopiera för annan AI',exact:true}).click();await workspace().getByRole('button',{name:'Kopiera visad text',exact:true}).click();await page.waitForFunction(()=>fixture.clipboard?.includes('Det hjälpte inte'));
  await workspace().getByRole('button',{name:'Klistra in AI-svar',exact:true}).click();await workspace().getByLabel('AI-svar',{exact:true}).fill('Förberett testsvar att granska. Ingen kontaktuppgift angavs.');
  await page.getByRole('navigation',{name:'Huvudnavigation'}).getByRole('button',{name:'Ditt arbete',exact:true}).click();await page.getByText('FIKTIVT DEMO – skrivaren på plan två',{exact:true}).first().click();
  await workspace().getByRole('button',{name:'Klistra in AI-svar',exact:true}).click();assert.match(await workspace().getByLabel('AI-svar',{exact:true}).inputValue(),/Förberett testsvar/);
  await workspace().getByRole('button',{name:'Spara som separat ogranskat utkast',exact:true}).click();
  await draftFields().getByLabel('Inklistrat utkast',{exact:true}).fill('Manuellt rättat testsvar. Kontaktuppgifter saknas.');
  await workspace().getByRole('button',{name:'Förhandsvisa och kopiera/exportera',exact:true}).click();await workspace().getByRole('button',{name:'Spara fil',exact:true}).click();await page.waitForFunction(()=>fixture.exportedText?.includes('Manuellt rättat testsvar'));
  assert.equal(await page.evaluate(()=>JSON.stringify(fixture.jobs[0].transcript)),original);
  assert.equal(await page.evaluate(()=>fixture.calls.some(c=>c.startsWith('plugin:opener'))),false);
  fs.mkdirSync('docs/ui-templates',{recursive:true});await page.screenshot({path:'docs/ui-templates/workspace.png',fullPage:true});
  await page.setViewportSize({width:920,height:950});await page.screenshot({path:'docs/ui-templates/narrow.png',fullPage:true});
  await page.getByRole('navigation',{name:'Huvudnavigation'}).getByRole('button',{name:'Ditt arbete',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('.save-status')?.textContent.includes('Sparat på datorn'));
  await page.evaluate(()=>{fixture.jobs[0].transcript.utterances[0].text+=' En ny komplettering.';});
  await page.getByText('FIKTIVT DEMO – skrivaren på plan två',{exact:true}).first().click();await workspace().getByText(/Detta utkast hör till den sparade källkopian/).waitFor();assert.equal(await draftFields().getByRole('checkbox').isDisabled(),true);
  assert.deepEqual(errors,[]);console.log('PASS template drafts, errors, edit/reopen, snapshot, template revisions/import/export, clipboard handoff, external draft, export, unchanged transcript');
 }catch(e){console.error(errors);fs.mkdirSync('docs/ui-templates',{recursive:true});await page.screenshot({path:'docs/ui-templates/failure.png',fullPage:true});throw e;}finally{await browser.close();}
})();
