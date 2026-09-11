// Synthetic IPC integration tests. Run against npm run dev; never reads real app data.
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
function setup() {
  const dictation = {revision:1,phase:'idle',message:'Redo att diktera',supported:true,backend:'CPU',settings:{model:'small',saveHistory:false,enabled:true,autoInsert:true},entries:[
    {id:'session',createdAt:1789115400000,text:'Bara i sessionen',saved:false,delivery:'Texten finns här.'},
    {id:'saved',createdAt:1789115300000,text:'Sparat diktat att hitta igen',saved:true,delivery:'Texten finns här.'}
  ]};
  const stored = JSON.parse(sessionStorage.getItem('synthetic-store') || 'null');
  const f=window.fixture={jobs:stored?.jobs||[],dictation:stored?.dictation||dictation,versions:stored?.versions||{},calls:[],failSave:false,failDictation:false,analysis:null};
  const persist=()=>sessionStorage.setItem('synthetic-store',JSON.stringify(f));
  const review=snapshot=>({text:snapshot.text,snapshot,segments:[{text:snapshot.text,span:snapshot.spans.length?0:null,start:0,end:new TextEncoder().encode(snapshot.text).length,word:true,para:0}],spans:snapshot.spans.map((s,id)=>({...s,id,replacement:s.custom||'Person 1'})),counts:{},warnings:[]});
  mockWindows('main'); mockConvertFileSrc('windows');
  mockIPC(async(cmd,args)=>{
    f.calls.push(cmd);
    switch(cmd) {
      case 'begin_work':return String(Date.now())+'-'+Math.random();
      case 'forget_work':return;
      case 'cancel_work':return true;
      case 'plugin:app|version':return '0.5.0';
      case 'list_whisper_models':return [{id:'small',label:'Small',downloaded:true}];
      case 'list_summary_models':return [{id:'qwen2.5-3b',label:'Qwen',downloaded:true}];
      case 'list_summary_templates':return [{id:'protokoll',label:'Protokoll'}];
      case 'list_jobs':return structuredClone(f.jobs);
      case 'search_jobs':return structuredClone(f.jobs.filter(j=>JSON.stringify(j).toLowerCase().includes((args.query||'').toLowerCase())));
      case 'list_all_actions':case 'list_actions':case 'list_standalone_tasks':return [];
      case 'dictation_snapshot':return structuredClone(f.dictation);
      case 'edit_dictation': {
        if(f.failDictation) throw Error('Test: diktatet kunde inte sparas.');
        const entry=f.dictation.entries.find(e=>e.id===args.id);
        if(args.expectedText!==undefined&&entry.text!==args.expectedText)throw Error('Diktatet har ändrats sedan förslaget skapades.');
        entry.originalText??=entry.text;
        Object.assign(f.dictation.entries.find(e=>e.id===args.id),{text:args.text,saved:args.saved});
        f.dictation.revision++; persist();
        await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{event:'avskrift:dictation',payload:structuredClone(f.dictation)}); return;
      }
      case 'save_job': {
        if(f.failSave) throw Error('Test: skrivfel');
        const old=f.jobs.find(j=>j.id===args.job.id);
        const j=structuredClone(args.job);j.originalSourceText=old?.originalSourceText??j.originalSourceText;
        f.jobs=f.jobs.filter(j=>j.id!==args.job.id).concat(j);persist();return;
      }
      case 'open_job':return structuredClone(f.jobs.find(j=>j.id===args.id));
      case 'checkpoint_job': (f.versions[args.id]??=[]).push(structuredClone(f.jobs.find(j=>j.id===args.id)));persist();return;
      case 'list_job_versions':return (f.versions[args.id]||[]).map((j,i)=>({id:String(i),title:j.title,updatedAt:j.updatedAt}));
      case 'open_job_version':return structuredClone(f.versions[args.id][Number(args.version)]);
      case 'restore_job_version': {
        const restored=structuredClone(f.versions[args.id][Number(args.version)]);
        f.versions[args.id].push(structuredClone(f.jobs.find(j=>j.id===args.id)));
        f.jobs=f.jobs.filter(j=>j.id!==args.id).concat(restored);persist();return;
      }
      case 'analyze_document': f.analysis=review({text:args.args.text,spans:[],paraRanges:[[0,new TextEncoder().encode(args.args.text).length]]});return structuredClone(f.analysis);
      case 'add_manual_span':f.analysis=review({...f.analysis.snapshot,spans:[{...args.args,text:f.analysis.text,source:'manual'}]});return structuredClone(f.analysis);
      case 'restore_review':f.analysis=review(args.snapshot);return structuredClone(f.analysis);
      case 'copy_anonymized':return f.analysis.snapshot.spans[0]?.custom||f.analysis.text;
      case 'rewrite_dictation':f.lastRewrite=structuredClone(args.args);return 'Ett tydligt mejl om beställningen.';
      case 'create_grounded_draft':f.lastSources=structuredClone(args.args.sources);return {discarded:1,items:[{kind:'decision',text:'Vi köper två böcker.',sourceId:args.args.sources[0].id,quote:'Vi köper två böcker.'},{kind:'action',text:'Åsa beställer på fredag.',sourceId:args.args.sources[1].id,quote:'Åsa beställer på fredag.'}]};
      default:return null;
    }
  },{shouldMockEvents:true});
}
(async()=>{
  const browser=await chromium.launch({headless:true,channel:'msedge'});
  const page=await browser.newPage({viewport:{width:1180,height:800}});
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(mocks+'\n('+setup.toString()+')();');
  const nav=()=>page.getByRole('navigation',{name:'Huvudnavigation'});
  const saved=()=>page.locator('.save-status').filter({hasText:'Sparat på datorn'}).waitFor();
  fs.mkdirSync('docs/ui-step2',{recursive:true});
  try {
    await page.goto('http://127.0.0.1:1420');
    await page.getByRole('heading',{name:'Ditt arbete',exact:true}).waitFor();
    assert.equal(await page.locator('.saved-dictations .job-row').count(),1);
    await page.locator('.saved-dictations .job-row').click();
    const dictat=page.locator('article[data-entry-id="saved"] textarea');
    await dictat.waitFor({state:'visible'});
    assert.equal(await dictat.evaluate(el=>document.activeElement===el),true);
    await dictat.fill('Diktat ändrat precis före navigering');
    await nav().getByRole('button',{name:'Ditt arbete',exact:true}).click();
    await page.waitForFunction(()=>fixture.dictation.entries.find(e=>e.id==='saved').text==='Diktat ändrat precis före navigering');
    await page.locator('.saved-dictations .job-row').click();
    await page.evaluate(()=>fixture.failDictation=true);
    await dictat.fill('Behåll min text vid skrivfel');
    await page.getByRole('button',{name:'Nytt arbete',exact:true}).click();
    await page.getByText('Error: Test: diktatet kunde inte sparas.',{exact:true}).waitFor();
    assert.equal(await dictat.inputValue(),'Behåll min text vid skrivfel');
    await page.evaluate(()=>fixture.failDictation=false);
    await page.getByRole('button',{name:'Nytt arbete',exact:true}).click();
    await page.getByRole('heading',{name:'Ditt arbete',exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>fixture.dictation.entries.find(e=>e.id==='session').saved),false);
    await nav().getByRole('button',{name:'Avidentifiering',exact:true}).click();
    await page.getByLabel('Källtext',{exact:true}).fill('Åsa bor i Umeå.');
    await saved();
    assert.equal(await page.evaluate(()=>fixture.jobs[0].sourceText),'Åsa bor i Umeå.');
    assert.equal(await page.evaluate(()=>fixture.calls.includes('analyze_document')),false);
    await page.reload();
    await page.getByRole('button',{name:/Åsa bor i Umeå/}).click();
    assert.equal(await page.getByLabel('Källtext',{exact:true}).inputValue(),'Åsa bor i Umeå.');
    await page.getByRole('button',{name:'Avidentifiera',exact:true}).click();
    await page.locator('.maskword').click();
    await page.locator('.modal input').fill('Eleven');
    await page.getByRole('button',{name:'Maskera',exact:true}).click();
    await saved();
    assert.equal(await page.evaluate(()=>fixture.jobs[0].reviewSnapshot.spans[0].custom),'Eleven');
    // Refresh simulates reopening a persisted project; restoring must not run analysis again.
    await page.reload();
    await page.getByRole('button',{name:/Åsa bor i Umeå/}).click();
    await page.waitForFunction(()=>fixture.calls.includes('restore_review'));
    assert.equal(await page.evaluate(()=>fixture.calls.includes('analyze_document')),false);
    await page.getByRole('button',{name:'Exportera…',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('#export-preview')?.value==='Eleven');
    await page.keyboard.press('Escape');
    await page.getByLabel('Källtext',{exact:true}).fill('Östen bor i Luleå.');
    await saved();
    await page.getByText('Underlaget har ändrats. Granskningen nedan',{exact:false}).waitFor();
    await page.getByRole('button',{name:'Kopiera för AI',exact:true}).click();
    assert.equal(await page.locator('.modal').count(),0);
    // Explicit processing archives the current review and source before replacing them.
    await page.getByRole('button',{name:'Kör om avidentifiering',exact:true}).click();
    await saved();
    await page.getByRole('button',{name:'Original och versioner',exact:true}).click();
    const versions=page.getByRole('dialog',{name:'Original och versioner'});
    await versions.waitFor();
    assert.equal(await versions.getByRole('textbox').inputValue(),'Åsa bor i Umeå.');
    await versions.getByRole('navigation',{name:'Sparade versioner'}).getByRole('button').nth(1).click();
    await versions.getByRole('button',{name:'Återställ denna version'}).waitFor();
    await page.screenshot({path:'docs/ui-step2/versions.png',fullPage:true});
    const before=await page.evaluate(()=>Object.values(fixture.versions)[0].length);
    await versions.getByRole('button',{name:'Återställ denna version'}).click();
    await versions.waitFor({state:'hidden'});
    assert.equal(await page.getByLabel('Källtext',{exact:true}).inputValue(),'Åsa bor i Umeå.');
    assert.equal(await page.evaluate(()=>Object.values(fixture.versions)[0].length),before+1);
    await page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Bibliotek',exact:true}).click();
    await page.locator('.saved-dictations .job-row').waitFor();
    await page.screenshot({path:'docs/ui-step2/library.png',fullPage:true});
    await page.setViewportSize({width:390,height:844});
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1 && document.querySelector('.app-content').scrollWidth<=document.querySelector('.app-content').clientWidth+1),'Library fits a narrow window');
    await page.screenshot({path:'docs/ui-step2/library-narrow.png',fullPage:true});
    await page.setViewportSize({width:1180,height:800});
    await page.evaluate(()=>{
      for(const id of ['möte-a','möte-b']) fixture.jobs.push({version:1,id,jobType:'meeting',title:id,createdAt:'2026-09-11',updatedAt:'2026-09-11',transcript:{language:'sv',model:'small',diarized:false,utterances:[{start:0,end:1,text:'Första underlaget',speaker:null,words:[]}]},summaryDraft:'Utkast',summaryBasis:JSON.stringify({source:'transcript',text:'Första underlaget'})});
    });
    await nav().getByRole('button',{name:'Ditt arbete',exact:true}).click();
    await page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Bibliotek',exact:true}).click();
    await page.getByRole('button',{name:/möte-a/}).click();
    await page.getByRole('button',{name:'Transkript',exact:true}).click();
    await page.locator('.utext .body').dblclick();
    await page.locator('textarea.edit').fill('Ett rättat underlag');
    await page.locator('textarea.edit').press('Control+Enter');
    await saved();
    assert.equal(await page.getByRole('button',{name:'Ångra',exact:true}).isEnabled(),true);
    await page.getByRole('button',{name:'Original och versioner',exact:true}).click();
    assert.equal(await page.getByRole('dialog',{name:'Original och versioner'}).getByRole('textbox').inputValue(),'Första underlaget');
    await page.keyboard.press('Escape');
    await page.getByRole('button',{name:'Sammanfattning',exact:true}).click();
    await page.getByText('Underlaget har ändrats sedan utkastet skapades.',{exact:false}).waitFor();
    await page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Bibliotek',exact:true}).click();
    await page.getByRole('button',{name:/möte-b/}).click();
    await page.getByRole('button',{name:'Transkript',exact:true}).click();
    assert.equal(await page.getByRole('button',{name:'Ångra',exact:true}).isDisabled(),true);
    assert.equal(await page.locator('.utext .body').textContent(),'Första underlaget');
    assert.deepEqual(errors,[]);
    console.log('PASS: source recovery, saved-only library, dictation drafts/failure recovery, manual-mask persistence, review restore without model, stale-review/summary guard, immutable originals, isolated undo history and version restore.');
  } catch(e) {
    console.error(errors); await page.screenshot({path:'docs/ui-step2/failure.png',fullPage:true});throw e;
  } finally {await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
