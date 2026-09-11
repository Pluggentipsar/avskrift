<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  type Model = { id:string;label:string;sizeMb:number;downloaded:boolean };
  let { models, textModels, speech = $bindable(), text = $bindable(), dictationModel, locked, downloading, percent, error,
    ondownload, ondictation, onchange, onclose }: {
    models:Model[];textModels:Model[];speech:string;text:string;dictationModel?:string;locked:boolean;downloading:string|null;percent:number;error:string;
    ondownload:(kind:'speech'|'text',id:string)=>Promise<void>;ondictation:(id:string)=>Promise<void>;onchange:()=>void;onclose:()=>void;
  } = $props();
  let dialog:HTMLDialogElement;
  let pending=$state(false), localError=$state('');
  type MemoryStatus = {busy:boolean;text:string|null;speech:string|null;memory:null|{ramFree:number|null;ramTotal:number|null;gpus:{name:string;free:number;total:number;integrated:boolean}[]}};
  let memoryStatus=$state<MemoryStatus|null>(null), memoryError=$state(false);
  const gib=(bytes:number)=>(bytes/1024**3).toLocaleString('sv-SE',{maximumFractionDigits:1});
  onMount(()=>{
    dialog.showModal();
    let alive=true,reading=false;
    async function refresh(){
      if(reading)return;reading=true;
      try{const value=await invoke<MemoryStatus>('runtime_memory_status');if(alive){memoryStatus=value;memoryError=false;}}
      catch{if(alive)memoryError=true;}finally{reading=false;}
    }
    void refresh();const timer=setInterval(refresh,5000);
    return ()=>{alive=false;clearInterval(timer);};
  });
  async function changeDictation(id:string){pending=true;localError='';try{await ondictation(id);}catch(e){localError=String(e);}finally{pending=false;}}
</script>
<dialog bind:this={dialog} aria-labelledby="models-title" onclose={onclose} oncancel={e=>{if(pending)e.preventDefault();}}>
  <header><div><h2 id="models-title">Modeller på datorn</h2><p>Välj och hämta modeller för ditt arbete.</p></div><button aria-label="Stäng modellinställningar" onclick={()=>dialog.close()} disabled={pending}>×</button></header>
  <p class="intro">Hämtning kräver internet. När modellerna finns på datorn bearbetas tal och text lokalt. Modellval sparas direkt.</p>
  {#if locked}<p role="status" class="notice">Ett arbete pågår. Du kan ändra modell när bearbetningen eller inspelningen är klar.</p>{/if}
  {#if error||localError}<p role="alert" class="error">{localError||error}</p>{/if}
  {#snippet selector(kind:'speech'|'text',id:string,available:Model[])}
    {@const chosen=available.find(m=>m.id===id)}
    <div class="model-status">
      {#if chosen?.downloaded}<span class="ready">Finns på datorn</span>
      {:else if downloading===id}<span role="status">Hämtar {percent}%</span><progress value={percent} max="100" aria-label="Modellhämtning"></progress>
      {:else}<span>Behöver hämtas{chosen?.sizeMb ? ` · ${chosen.sizeMb} MB` : ''}</span><button onclick={()=>ondownload(kind,id)} disabled={!chosen||!!downloading||locked||pending}>Hämta modell</button>{/if}
    </div>
  {/snippet}
  <section><div><h3>Möten och ljudfiler</h3><p>Talmodellen används vid nästa transkribering. Sparad text ändras inte av modellvalet.</p></div><div>
    <label for="speech-model">Talmodell för möten</label><select id="speech-model" bind:value={speech} onchange={()=>queueMicrotask(onchange)} disabled={locked||pending}>
      {#if !models.some(m=>m.id===speech)}<option value={speech}>{speech} – inte tillgänglig</option>{/if}
      {#each models as m}<option value={m.id}>{m.label}</option>{/each}</select>{@render selector('speech',speech,models)}
  </div></section>
  <section><div><h3>Diktering</h3><p>Ett eget modellval för korta diktat. Hämtade talmodeller delas med mötesfunktionen.</p></div><div>
    <label for="dictation-model">Talmodell för diktering</label><select id="dictation-model" value={dictationModel??''} onchange={e=>{const select=e.currentTarget;void changeDictation(select.value).finally(()=>select.value=dictationModel??'');}} disabled={locked||pending||!dictationModel}>
      {#if !models.some(m=>m.id===dictationModel)}<option value={dictationModel??''}>{dictationModel??'Diktering är inte tillgänglig'}</option>{/if}
      {#each models as m}<option value={m.id}>{m.label}</option>{/each}</select>{#if dictationModel}{@render selector('speech',dictationModel,models)}{/if}
  </div></section>
  <section><div><h3>Sammanfattning och textbearbetning</h3><p>Gemensamt val för källutkast, fria sammanfattningar, frågor, åtgärdsförslag och bearbetning av diktat. Ett öppnat projekt kan återställa sitt sparade textmodellval.</p></div><div>
    <label for="text-model">Textmodell</label><select id="text-model" bind:value={text} onchange={()=>queueMicrotask(onchange)} disabled={locked||pending}>
      {#if !textModels.some(m=>m.id===text)}<option value={text}>{text} – inte tillgänglig</option>{/if}
      {#each textModels as m}<option value={m.id}>{m.label}</option>{/each}</select>{@render selector('text',text,textModels)}
  </div></section>
  <details class="memory"><summary>Minne och bearbetning</summary>
    <p>AVskrift anpassar modellernas GPU-användning efter tillgängligt minne. Inaktiva modeller frigörs efter ungefär två minuter och laddas igen när de behövs. Dina texter och modellfiler finns kvar.</p>
    {#if memoryError}<p>Minnesstatus kunde inte läsas just nu.</p>
    {:else if !memoryStatus}<p>Läser minnesstatus…</p>
    {:else if memoryStatus.busy}<p role="status">Modellarbete pågår. Minnesläget uppdateras när motorn är ledig.</p>
    {:else}
      {#if memoryStatus.memory?.ramFree!=null}<p>Ledigt arbetsminne: <strong>{gib(memoryStatus.memory.ramFree)} GB</strong>{#if memoryStatus.memory.ramTotal!=null}{' '}av {gib(memoryStatus.memory.ramTotal)} GB{/if}.</p>{/if}
      {#each memoryStatus.memory?.gpus??[] as gpu}<p>{gpu.name}: {#if gpu.total>0}{gib(gpu.free)} av {gib(gpu.total)} GB ledigt{:else}minnesuppgift saknas{/if}{gpu.integrated?' · delat minne':''}.</p>{/each}
      <p>Textmodell: {memoryStatus.text??'inte laddad'}.</p><p>Talmodell: {memoryStatus.speech??'inte laddad'}.</p>
    {/if}
    <p>Om minnet inte räcker: stäng andra program eller välj en mindre modell. CPU-reservvägen kan ta längre tid.</p>
  </details>
  <details><summary>Om modellernas uppgifter</summary><p>Mindre talmodeller använder mindre minne och brukar bli klara snabbare. Större modeller kan ge bättre text, men behöver mer resurser. Prova med ditt eget ljud.</p><p>Avidentifieringens identifieringsmodell och djupare AI-granskning använder separata modellval. När granskning och textbearbetning använder samma textmodell delar de dess laddade kopia. Textmodellvalet ovan ändrar inte granskningsmodellens val.</p></details>
  <footer><button onclick={()=>dialog.close()} disabled={pending}>Tillbaka till arbetet</button></footer>
</dialog>
<style>
  dialog{box-sizing:border-box;width:min(880px,calc(100vw - 32px));max-height:calc(100dvh - 32px);padding:28px;border:1px solid var(--line);border-radius:12px;background:var(--bg);color:var(--ink);font:14px/1.6 Archivo,sans-serif}dialog::backdrop{background:#17172b66}header{display:flex;justify-content:space-between;align-items:start;gap:20px}h2{font:34px 'Instrument Serif',serif;margin:0}h3{font-size:16px;margin:0 0 6px}p{color:var(--muted);margin:6px 0}header>button{font-size:22px;padding:0 12px}.intro{margin:20px 0}section{display:grid;grid-template-columns:1fr 1fr;gap:32px;border-top:1px solid var(--line);padding:24px 0}label{display:block;font-weight:500;margin-bottom:6px}select,button{font:inherit;color:var(--ink);background:var(--bg);border:1px solid var(--line-2);border-radius:6px;padding:9px 12px}select{width:100%;box-sizing:border-box}button{cursor:pointer}button:disabled,select:disabled{opacity:.55}.model-status{display:flex;align-items:center;justify-content:space-between;gap:10px;flex-wrap:wrap;font-size:12px;margin-top:10px;color:var(--muted)}.ready{color:var(--accent)}progress{max-width:130px;accent-color:var(--accent)}.notice{padding:12px;background:var(--accent-soft)}.error{color:#923115}details{border-top:1px solid var(--line);padding-top:18px}summary{cursor:pointer}footer{display:flex;justify-content:flex-end;margin-top:24px}:is(button,select,summary):focus-visible{outline:2px solid var(--accent);outline-offset:3px}@media(max-width:650px){section{grid-template-columns:1fr;gap:12px}dialog{padding:20px}}
</style>
