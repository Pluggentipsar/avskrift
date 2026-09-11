// Run against `npm run dev`. Mock IPC uses only synthetic fixtures; no app data or models.
const { chromium } = require(process.env.AVSKRIFT_PLAYWRIGHT || 'playwright');
const fs = require('node:fs');
const assert = require('node:assert/strict');
const mocks = fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
(async () => {
  const browser = await chromium.launch({headless:true, channel:'msedge'});
  const page = await browser.newPage({viewport:{width:1180,height:800}});
  const errors=[]; page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(mocks + `
    const sampleTranscript = {language:'sv',model:'kb-whisper-small',diarized:false,utterances:[{start:0,end:5,speaker:'Talare 1',text:'Vi beställer tio böcker.',words:[]}]};
    const sampleJob = {version:1,id:'fixture-meeting',jobType:'meeting',title:'Veckomöte på biblioteket',createdAt:'2026-09-11T08:00:00Z',updatedAt:'2026-09-11T08:00:00Z',transcript:sampleTranscript,speakerLabels:{'Talare 1':'Anna'},notes:'Kontrollera leveransen.',summaryDraft:'Biblioteket beställer tio böcker.',category:'',actions:[],participants:[],enabled:['person'],rejected:[]};
    window.fixture = {jobs:[sampleJob],writes:[],exports:[],failSave:false,previewDelay:0};
    mockWindows('main');
    mockConvertFileSrc('windows');
    mockIPC(async (cmd,args) => {
      const f=window.fixture;
      switch(cmd) {
        case 'begin_work':return String(Date.now())+'-'+Math.random();
      case 'forget_work':return;
      case 'cancel_work':return true;
      case 'plugin:app|version': return '0.5.0';
        case 'list_whisper_models': return [{id:'kb-whisper-small',label:'Small',sizeMb:480,downloaded:true}];
        case 'list_summary_models': return [{id:'qwen2.5-3b',label:'Qwen',sizeMb:2000,downloaded:true}];
        case 'list_summary_templates': return [{id:'protokoll',label:'Protokoll'}];
        case 'list_jobs': case 'search_jobs': return structuredClone(f.jobs);
        case 'list_all_actions': case 'list_actions': case 'list_standalone_tasks': return [];
        case 'open_job': return structuredClone(f.jobs.find(j=>j.id===args.id));
        case 'save_job':
          f.writes.push(structuredClone(args.job));
          if(f.failSave) throw new Error('Test: disken kunde inte skrivas.');
          f.jobs=f.jobs.filter(j=>j.id!==args.job.id).concat(structuredClone(args.job)); return;
        case 'dictation_snapshot': return {revision:1,phase:'idle',message:'Redo att diktera',supported:true,backend:'CPU',settings:{model:'kb-whisper-small',saveHistory:false,enabled:true,autoInsert:true},holdShortcutReady:true,toggleShortcutReady:true,entries:[{id:'dictat-1',createdAt:1789115400000,text:'Kan du bekräfta beställningen?',saved:false,delivery:'Inget textfält var aktivt. Texten finns kvar här.'}]};
        case 'update_transcript': return;
        case 'preview_transcript':
          if(f.previewDelay) await new Promise(r=>setTimeout(r,f.previewDelay));
          return args.args.path.endsWith('.srt')?'1\\n00:00:00,000 --> 00:00:05,000\\nAnna: Vi beställer tio böcker.\\n':'Anna: Vi beställer tio böcker.\\n';
        case 'analyze_document': return {text:args.args.text,segments:[{text:args.args.text,span:null,start:0,end:args.args.text.length,word:true,para:0}],spans:[],counts:{},warnings:[]};
        case 'copy_anonymized': return 'Person 1 beställer böcker.';
        case 'plugin:dialog|save': return 'C:/synthetic/export.txt';
        case 'save_summary': f.exports.push(args.args);return;
        default: return null;
      }
    },{shouldMockEvents:true});
  `);
  try {
    await page.goto('http://127.0.0.1:1420');
    await page.getByRole('heading',{name:'Ditt arbete',exact:true}).waitFor();
    assert.equal(await page.locator('.entrypoints button').count(),3);
    fs.mkdirSync('docs/ui-step1',{recursive:true});
    await page.screenshot({path:'docs/ui-step1/home.png',fullPage:true});
    await page.getByRole('button',{name:/Veckomöte på biblioteket/}).click();
    const draft=page.getByLabel('Sammanfattning – redigerbart utkast');
    await draft.fill('Ett nytt utkast som ska sparas.');
    await page.locator('.save-status').filter({hasText:'Sparat på datorn'}).waitFor();
    assert.equal(await page.evaluate(()=>fixture.jobs[0].summaryDraft),'Ett nytt utkast som ska sparas.');
    await page.evaluate(()=>fixture.failSave=true);
    await draft.fill('Den här ändringen måste gå att rädda.');
    await page.getByText('Ändringarna kunde inte sparas på datorn.').waitFor();
    await page.getByRole('button',{name:'Nytt arbete',exact:true}).click();
    assert.equal(await draft.inputValue(),'Den här ändringen måste gå att rädda.');
    await page.evaluate(()=>fixture.failSave=false);
    await page.getByRole('button',{name:'Försök spara igen'}).click();
    await page.locator('.save-status').filter({hasText:'Sparat på datorn'}).waitFor();
    await page.getByRole('button',{name:'Exportera…',exact:true}).click();
    const dialog=page.getByRole('dialog',{name:'Granska och exportera'});
    await dialog.waitFor();
    await page.waitForFunction(()=>document.querySelector('#export-preview')?.value==='Den här ändringen måste gå att rädda.');
    await dialog.getByLabel('Format',{exact:true}).selectOption('docx');
    await page.waitForFunction(()=>!document.querySelector('#export-preview')?.getAttribute('aria-busy') || document.querySelector('#export-preview')?.getAttribute('aria-busy')==='false');
    await dialog.getByRole('button',{name:'Spara fil…'}).click();
    await dialog.getByText('Filen är sparad på datorn.').waitFor();
    assert.equal(await page.evaluate(()=>fixture.exports[0].text),'Den här ändringen måste gå att rädda.');
    assert.equal(await page.evaluate(()=>fixture.exports[0].includeTranscript),false);
    await page.screenshot({path:'docs/ui-step1/export.png',fullPage:true});
    await page.keyboard.press('Escape');
    await dialog.waitFor({state:'hidden'});
    await page.getByRole('button',{name:'Anteckningar och åtgärder',exact:true}).click();
    await page.locator('.ws-notes').fill('Anteckning precis före projektbyte.');
    await page.getByRole('button',{name:'Nytt arbete',exact:true}).click();
    await page.getByRole('heading',{name:'Ditt arbete',exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>fixture.jobs[0].notes),'Anteckning precis före projektbyte.');
    await page.getByRole('navigation',{name:'Huvudnavigation'}).getByRole('button',{name:'Diktering',exact:true}).click();
    await page.getByText('Senaste diktatet',{exact:true}).waitFor();
    assert.equal(await page.locator('.settings').getAttribute('open'),null);
    await page.screenshot({path:'docs/ui-step1/dictation.png',fullPage:true});
    await page.getByRole('navigation',{name:'Huvudnavigation'}).getByRole('button',{name:'Avidentifiering',exact:true}).click();
    await page.locator('.src-text').fill('Anna beställer böcker.');
    await page.getByRole('button',{name:'Avidentifiera',exact:true}).click();
    await page.getByRole('button',{name:'Exportera…',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('#export-preview')?.value==='Person 1 beställer böcker.');
    await page.keyboard.press('Escape');
    await page.getByRole('navigation',{name:'Huvudnavigation'}).getByRole('button',{name:'Ditt arbete',exact:true}).click();
    await page.setViewportSize({width:390,height:844});
    await page.screenshot({path:'docs/ui-step1/narrow.png',fullPage:true});
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth <= innerWidth+1),'No horizontal overflow at 390px');
    assert.deepEqual(errors,[]);
    console.log('PASS: navigation, autosave, failed save/retry, safe reset, snapshot export, Escape, dictation, de-identification, narrow layout.');
  } catch(e) {
    console.error('Browser errors:',errors);
    console.error('Save state:', await page.locator('.save-status').allTextContents());
    console.error('Synthetic writes:', await page.evaluate(()=>fixture.writes));
    await page.screenshot({path:'docs/ui-step1/failure.png',fullPage:true});
    throw e;
  } finally { await browser.close(); }
})().catch(e=>{console.error(e);process.exitCode=1;});
