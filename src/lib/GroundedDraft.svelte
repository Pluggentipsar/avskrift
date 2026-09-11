<script lang="ts">
  import {invokeWork,isWorkCancelled} from "$lib/work";
  import {invoke} from '@tauri-apps/api/core';
  import TemplatePicker from './TemplatePicker.svelte';
  import {hasSource,type SourceUnit,type GroundedWork,type DraftItem} from './grounded';
  let {value,sources,context,model,canListen=false,disabled=false,busy=$bindable(false),before,onchange,onseek,onaction,ondecision,onuse}: {
    value:GroundedWork|null;sources:SourceUnit[];context:string;model:string;canListen?:boolean;disabled?:boolean;busy?:boolean;
    before:()=>Promise<boolean>;onchange:(v:GroundedWork)=>void;onseek:(source:SourceUnit)=>void;
    onaction:(text:string,source?:{quote:string;start:number|null})=>void;ondecision?:(text:string,source:{quote:string;start:number|null})=>void;onuse:(text:string)=>void;
  }=$props();
  let instructions=$state(''),template=$state(''),error=$state(''),selected=$state<number|null>(null);
  const stale=$derived(!!value&&(value.context!==context||JSON.stringify(value.sources)!==JSON.stringify(sources)));
  const chosen=$derived(value&&selected!==null?value.items[selected]:null);
  const source=$derived(chosen&&value?value.sources.find(s=>s.id===chosen.sourceId):null);
  const approved=$derived(value?.items.filter(i=>i.approved&&hasSource(i,value!.sources))??[]);
  async function generate(){
    if(busy||disabled||!sources.length)return;
    busy=true;error='';
    try{if(!(await before()))return;
      const captured=JSON.parse(JSON.stringify(sources));const capturedContext=context;
      const result=await invokeWork<{items:DraftItem[];discarded:number}>('create_grounded_draft',{args:{sources:captured,model,instructions}});
      onchange({sources:captured,context:capturedContext,items:result.items.map(i=>({...i,generatedText:i.text,approved:false,added:false})),discarded:result.discarded,template,createdAt:new Date().toISOString()});selected=null;
    }catch(e){error=String(e);}finally{busy=false;}
  }
  function change(index:number,patch:Partial<DraftItem>){if(!value)return;onchange({...value,items:value.items.map((i,k)=>k===index?{...i,...patch}:i)});}
  function add(index:number){const item=value?.items[index];if(!item||!value||stale||!item.approved||item.added||!hasSource(item,value.sources))return;onaction(item.text,{quote:item.quote,start:value.sources.find(s=>s.id===item.sourceId)?.start??null});change(index,{added:true});}
  function addDecision(index:number){const item=value?.items[index];if(!item||!value||stale||!item.approved||item.added||!hasSource(item,value.sources))return;ondecision?.(item.text,{quote:item.quote,start:value.sources.find(s=>s.id===item.sourceId)?.start??null});change(index,{added:true});}
  function use(){if(!stale&&approved.length)onuse(approved.map(i=>`- ${i.text}`).join('\n'));}
</script>
<section class="grounded" aria-label="Källbelagt utkast">
  <div class="heading"><div><h2>Från underlag till utkast</h2><p>Öppna citatet, kontrollera betydelsen och godkänn de punkter du vill använda.</p></div><button class="primary" onclick={generate} disabled={busy||disabled||!sources.length||!instructions}>{busy?'Arbetar…':value?'Skapa nytt källutkast':'Skapa källutkast'}</button></div>
  <TemplatePicker scope="meeting" disabled={busy} onchange={(text,label)=>{instructions=text;template=label;}} />
  {#if error}<p role={isWorkCancelled(error)?"status":"alert"} class:warning={!isWorkCancelled(error)}>{error}</p>{/if}
  {#if value}
    {#if stale}<p class="warning" role="status">Underlaget har ändrats. Du kan läsa de tidigare citaten, men behöver skapa ett nytt källutkast innan punkterna används.</p>{/if}
    {#if value.discarded}<p class="warning">{value.discarded} förslag saknade en giltig citathänvisning och togs bort.</p>{/if}
    <p class="hint">{value.template} · Ett hittat citat bekräftar var texten finns. Du behöver fortfarande kontrollera att det stöder förslaget.</p>
    <div class="columns"><aside aria-label="Källutdrag"><h3>Underlag</h3>
      {#if source&&chosen}<p class="hint">{source.start===null?'Textutdrag':`Vid ${Math.floor(source.start/60)}:${Math.floor(source.start%60).toString().padStart(2,'0')}`}</p><blockquote>{chosen.quote}</blockquote><p class="source">{source.text}</p>
        {#if source.start!==null}<button onclick={()=>source&&onseek(source)} disabled={stale||busy}>{canListen?'Öppna och lyssna':'Öppna transkriptet'}</button>{/if}
      {:else}<p class="hint">Välj ”Visa källa” vid en punkt för att läsa underlaget här.</p>{/if}
    </aside><div class="items"><h3>Förslag att granska</h3>
      {#if !value.items.length}<p>Inga punkter med giltiga citat hittades. Prova en annan mall eller ett tydligare underlag.</p>{/if}
      {#each value.items as item,index}<article class:chosen={selected===index}>
        <div class="item-head"><span>{item.kind==='action'?'Föreslagen åtgärd':item.kind==='decision'?'Uppgift om beslut':'Anteckning'}</span><span>{item.approved?'Godkänt av dig':item.text!==item.generatedText?'Redigerat av dig':'AI-förslag'}</span></div>
        <textarea aria-label="Förslag {index+1}" value={item.text} oninput={e=>change(index,{text:e.currentTarget.value,approved:false})} disabled={busy||stale||item.added} rows="3"></textarea>
        <div class="controls"><button onclick={()=>selected=index} disabled={!hasSource(item,value.sources)}>Visa källa</button><label><input type="checkbox" checked={!!item.approved} onchange={e=>change(index,{approved:e.currentTarget.checked})} disabled={busy||stale||item.added||!item.text.trim()||!hasSource(item,value.sources)} /> Jag har granskat punkten</label>
          {#if item.kind==='decision'&&ondecision}<button onclick={()=>addDecision(index)} disabled={busy||stale||!item.approved||item.added}>{item.added?'Tillagt i beslut':'Lägg till beslut'}</button>{/if}
          {#if item.kind==='action'}<button onclick={()=>add(index)} disabled={busy||stale||!item.approved||item.added}>{item.added?'Tillagd i åtgärder':'Lägg till åtgärd'}</button>{/if}
        </div>
      </article>{/each}
    </div></div>
    <div class="foot"><span>{approved.length} av {value.items.length} punkter godkända</span><button class="primary" onclick={use} disabled={busy||stale||!approved.length}>Använd godkända punkter som utkast</button></div>
  {/if}
</section>
<style>
  .grounded{border:1px solid var(--line);border-radius:10px;padding:24px;margin-bottom:24px;background:var(--bg);font:14px/1.6 Archivo,sans-serif}.heading,.controls,.item-head,.foot{display:flex;gap:14px;align-items:center;justify-content:space-between;flex-wrap:wrap}h2{font:30px/1.2 'Instrument Serif',serif;margin:0}h3{font-size:15px;margin:0 0 14px}p{margin:10px 0 18px}.heading p,.hint,.foot{color:var(--muted)}.heading{margin-bottom:20px}button,textarea{font:inherit;color:var(--ink);background:var(--bg);border:1px solid var(--line-2);border-radius:6px;padding:9px 12px;box-sizing:border-box}button{cursor:pointer}.primary{background:var(--accent);color:white;border-color:var(--accent)}button:disabled,textarea:disabled{opacity:.55}textarea{width:100%;resize:vertical;margin:8px 0}.columns{display:grid;grid-template-columns:minmax(210px,.8fr) minmax(280px,1.2fr);gap:24px;margin-top:22px}aside{border-right:1px solid var(--line);padding-right:24px;overflow-wrap:anywhere}.source{white-space:pre-wrap;max-height:420px;overflow:auto}blockquote{margin:0;border-left:3px solid var(--accent);padding:12px 16px;background:var(--accent-soft);white-space:pre-wrap}.item-head{font-size:12px;color:var(--muted)}article{padding:16px 0;border-bottom:1px solid var(--line)}article.chosen{border-left:3px solid var(--accent);padding-left:12px}.controls{justify-content:flex-start;font-size:13px}.controls label{display:flex;gap:8px;align-items:center}.foot{margin-top:20px}.warning{color:#865713;background:#fff6df;padding:12px}.hint{font-size:13px}:is(button,textarea,input):focus-visible{outline:2px solid var(--accent);outline-offset:3px}@media(max-width:900px){.columns{grid-template-columns:1fr}aside{border-right:0;border-bottom:1px solid var(--line);padding:0 0 20px}.grounded{padding:16px}}
</style>
