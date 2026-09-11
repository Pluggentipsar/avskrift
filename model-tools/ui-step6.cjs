// Synthetic memory-status IPC only. No real device queries or project data.
const {chromium}=require(process.env.AVSKRIFT_PLAYWRIGHT||'playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const mocks=fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
const base=fs.readFileSync('model-tools/ui-step2.cjs','utf8');
let setup=base.slice(base.indexOf('function setup()'),base.indexOf('\n(async()=>'));
setup=setup.replace("case 'plugin:app|version':",`case 'runtime_memory_status':
  if(f.memoryFail)throw Error('synthetic error');
  return structuredClone(f.memory??{busy:false,text:null,speech:null,memory:{ramFree:4*1024**3,ramTotal:8*1024**3,gpus:[]}});
  case 'plugin:app|version':`);
(async()=>{
 const browser=await chromium.launch({headless:true,channel:'msedge'});
 const page=await browser.newPage({viewport:{width:1440,height:1050}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(mocks+'\n'+setup+'\nsetup();');
 const open=()=>page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Modeller på datorn'}).click();
 const dialog=page.getByRole('dialog',{name:'Modeller på datorn'});
 fs.mkdirSync('docs/ui-step6',{recursive:true});
 try{
  await page.goto('http://127.0.0.1:1420');await page.getByRole('heading',{name:'Ditt arbete',exact:true}).waitFor();
  await open();await dialog.getByText('Minne och bearbetning',{exact:true}).click();
  await dialog.getByText('Textmodell: inte laddad.',{exact:true}).waitFor();
  assert.match(await dialog.innerText(),/Ledigt arbetsminne: 4 GB av 8 GB/);
  await page.evaluate(()=>fixture.memory={busy:true,memory:null,text:null,speech:null});
  await dialog.getByText('Modellarbete pågår. Minnesläget uppdateras när motorn är ledig.',{exact:true}).waitFor();
  await page.evaluate(()=>fixture.memory={busy:false,memory:{ramFree:6*1024**3,ramTotal:16*1024**3,gpus:[{name:'Syntetisk GPU',free:3*1024**3,total:8*1024**3,integrated:false}]},text:'CPU – reservväg efter GPU-fel',speech:'GPU'});
  await dialog.getByText('Textmodell: CPU – reservväg efter GPU-fel.',{exact:true}).waitFor();
  assert.match(await dialog.innerText(),/Syntetisk GPU: 3 av 8 GB ledigt/);
  await page.screenshot({path:'docs/ui-step6/memory.png',fullPage:true});
  await page.evaluate(()=>{fixture.memory.memory.gpus[0].total=0;fixture.memory.memory.gpus[0].free=0;});
  await dialog.getByText('Syntetisk GPU: minnesuppgift saknas.',{exact:true}).waitFor();
  await page.evaluate(()=>fixture.memoryFail=true);
  await dialog.getByText('Minnesstatus kunde inte läsas just nu.',{exact:true}).waitFor();
  await dialog.getByRole('button',{name:'Tillbaka till arbetet',exact:true}).click();
  const reads=await page.evaluate(()=>fixture.calls.filter(c=>c==='runtime_memory_status').length);
  await page.waitForTimeout(5500);
  assert.equal(await page.evaluate(()=>fixture.calls.filter(c=>c==='runtime_memory_status').length),reads);
  assert.deepEqual(errors,[]);
  console.log('PASS: RAM/device display, empty cache, active-work state, CPU recovery status, status errors and stopped polling after close.');
 }finally{await browser.close();}
})();
