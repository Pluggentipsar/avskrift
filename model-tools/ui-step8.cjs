// Synthetic library IPC. No real project files, audio or user search terms.
const {chromium}=require(process.env.AVSKRIFT_PLAYWRIGHT||'playwright');
const fs=require('node:fs'),assert=require('node:assert/strict');
const mocks=fs.readFileSync('node_modules/@tauri-apps/api/mocks.js','utf8').replace(/export \{[^}]+\};?/g,'');
const base=fs.readFileSync('model-tools/ui-step2.cjs','utf8');
let setup=base.slice(base.indexOf('function setup()'),base.indexOf('\n(async()=>'));
setup=setup.replace("case 'search_jobs':return",`case 'search_jobs':
  if(f.failSearch){f.failSearch=false;throw Error('syntetiskt sökfel');}
  if(f.holdSearch)return new Promise((resolve,reject)=>{(f.searches??=[]).push({query:args.query,resolve,reject});});
  return`);
setup=setup.replace("case 'list_jobs':return",`case 'refresh_library':
  if(f.failRefresh){f.failRefresh=false;throw Error('syntetiskt läsfel');}
  return new Promise((resolve,reject)=>{f.refresh={resolve,reject};});
  case 'list_jobs':return`);
(async()=>{
 const browser=await chromium.launch({headless:true,channel:'msedge'});
 const page=await browser.newPage({viewport:{width:1440,height:960}}),errors=[];
 page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(mocks+'\n'+setup+'\nsetup();');
 const library=()=>page.getByRole('navigation',{name:'Bibliotek och verktyg'}).getByRole('button',{name:'Bibliotek',exact:true}).click();
 const search=page.getByRole('searchbox',{name:'Sök bland projekten'});
 const pending=n=>page.waitForFunction(n=>(fixture.searches?.length??0)>=n,n);
 const reply=(i,fail=false)=>page.evaluate(({i,fail})=>{
   const r=fixture.searches[i];
   fail?r.reject('Ett gammalt fel'):r.resolve(structuredClone(fixture.jobs.filter(j=>JSON.stringify(j).toLowerCase().includes(r.query.toLowerCase()))));
 },{i,fail});
 fs.mkdirSync('docs/ui-step8',{recursive:true});
 try {
   await page.goto('http://127.0.0.1:1420');await page.getByRole('heading',{name:'Ditt arbete',exact:true}).waitFor();
   await page.evaluate(()=>fixture.jobs.push(
     {version:2,id:'asa',jobType:'summarize',title:'Åsas anteckning',category:'Skola',createdAt:'2026-09-11',updatedAt:'2026-09-11',sourceText:'Böcker på biblioteket',summaryDraft:'Ett bevarat utkast'},
     {version:2,id:'osten',jobType:'summarize',title:'Östens möte',category:'Omsorg',createdAt:'2026-09-11',updatedAt:'2026-09-11',sourceText:'Uppföljning av mötet',summaryDraft:'Ett annat utkast'}
   ));
   await library();await page.getByRole('button',{name:/Åsas anteckning/}).waitFor();
   await page.evaluate(()=>fixture.holdSearch=true);
   await search.fill('Östen');await pending(1);
   await page.getByRole('status').filter({hasText:'Söker bland projekten…'}).waitFor();
   assert.equal(await page.getByText(/Inga träffar för/).count(),0);
   assert.equal(await page.locator('.job-item').count(),0); // no stale results labelled with a new query
   await search.fill('Åsa');await pending(2);await reply(1);
   await page.getByRole('button',{name:/Åsas anteckning/}).waitFor();
   assert.equal(await page.getByRole('button',{name:/Östens möte/}).count(),0);
   await reply(0,true);assert.equal(await page.getByRole('alert').count(),0);
   await page.evaluate(()=>{fixture.holdSearch=false;fixture.failSearch=true;});
   await search.fill('biblioteket');await page.getByRole('alert').filter({hasText:'Sökningen kunde inte slutföras'}).waitFor();
   assert.equal(await page.locator('.job-item').count(),0);
   await page.getByRole('button',{name:'Försök söka igen',exact:true}).click();
   await page.getByRole('button',{name:/Åsas anteckning/}).waitFor();
   const originals=await page.evaluate(()=>JSON.stringify(fixture.jobs));
   await page.getByRole('button',{name:'Uppdatera biblioteket',exact:true}).click();
   await page.waitForFunction(()=>!!fixture.refresh);
   assert.equal(await page.getByRole('button',{name:'Uppdaterar biblioteket…',exact:true}).isDisabled(),true);
   await search.fill('Östen');
   await page.evaluate(()=>{fixture.refresh.resolve(2);fixture.refresh=null;});
   await page.getByRole('button',{name:/Östens möte/}).waitFor();
   assert.equal(await search.inputValue(),'Östen');
   assert.equal(await page.evaluate(()=>JSON.stringify(fixture.jobs)),originals);
   await page.screenshot({path:'docs/ui-step8/library.png',fullPage:true});
   await page.evaluate(()=>fixture.failRefresh=true);
   await page.getByRole('button',{name:'Uppdatera biblioteket',exact:true}).click();
   await page.getByRole('alert').filter({hasText:'Biblioteket kunde inte uppdateras'}).waitFor();
   await page.getByRole('button',{name:/Östens möte/}).waitFor();
   assert.equal(await page.evaluate(()=>JSON.stringify(fixture.jobs)),originals);
   await search.fill('xyz-no-match');await page.getByText('Inga träffar för ”xyz-no-match”.',{exact:true}).waitFor();
   await search.fill('');await page.getByRole('button',{name:/Åsas anteckning/}).waitFor();
   await page.setViewportSize({width:390,height:844});
   assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1));
   await page.screenshot({path:'docs/ui-step8/narrow.png',fullPage:true});
   assert.deepEqual(errors,[]);
   console.log('PASS: latest query wins, no false empty/stale results, search errors/retry, rebuild keeps current query and originals, rebuild error recovery, Swedish content search and narrow layout.');
 }catch(e){console.error(errors);await page.screenshot({path:'docs/ui-step8/failure.png',fullPage:true});throw e;}finally{await browser.close();}
})();
