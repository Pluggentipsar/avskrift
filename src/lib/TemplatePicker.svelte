<script lang="ts">
  import { onMount } from 'svelte';
  let { scope, onchange, disabled=false }: {scope:'meeting'|'dictation';onchange:(instructions:string,label:string)=>void;disabled?:boolean}=$props();
  type Template={id:string;label:string;instructions:string};
  const builtins:Record<string,Template[]>={
    meeting:[{id:'minutes',label:'Mötesprotokoll',instructions:'Sammanfatta ämnen, uttryckliga beslut och åtgärder. Skilj diskussion från fattade beslut.'},{id:'actions',label:'Beslut och åtgärder',instructions:'Ta endast med uttryckliga beslut och konkreta åtgärder som framgår av underlaget.'},{id:'brief',label:'Kort lägesbild',instructions:'Ge en kort lägesbild med de viktigaste sakuppgifterna.'}],
    dictation:[{id:'email',label:'Mejl',instructions:'Skriv ett kort, tydligt mejl med stycken. Lägg inte till mottagare, hälsningsfras med namn eller avsändare som saknas i underlaget.'},{id:'note',label:'Tjänsteanteckning',instructions:'Skriv en saklig tjänsteanteckning med korta stycken. Lägg inte till datum, namn eller bedömningar som saknas.'},{id:'clean',label:'Rätta och förtydliga',instructions:'Rätta stavning, skiljetecken och upprepningar. Bevara ordval och betydelse så långt det går.'}]
  };
  let custom=$state<Template[]>([]), selected=$state(''), name=$state(''), instructions=$state(''), error=$state('');
  const choices=$derived([...builtins[scope],...custom]);
  function choose(id:string) {selected=id;const t=choices.find(t=>t.id===id);if(t)onchange(t.instructions,t.label);}
  onMount(()=>{
    try{const data=JSON.parse(localStorage.getItem('avskrift_templates_'+scope)||'[]');if(Array.isArray(data))custom=data.filter(t=>typeof t.id==='string'&&typeof t.label==='string'&&typeof t.instructions==='string').slice(0,30);}
    catch{error='Sparade mallar kunde inte läsas. Standardmallarna går att använda.';}
    choose(builtins[scope][0].id);
  });
  function store(next:Template[]){try{localStorage.setItem('avskrift_templates_'+scope,JSON.stringify(next));custom=next;error='';return true;}catch{error='Mallen kunde inte sparas på datorn.';return false;}}
  function save(){if(!name.trim()||!instructions.trim())return;const t={id:crypto.randomUUID(),label:name.trim(),instructions:instructions.trim()};if(store([...custom,t])){choose(t.id);name='';instructions='';}}
  function remove(){if(store(custom.filter(t=>t.id!==selected)))choose(builtins[scope][0].id);}
</script>
<div class="templates">
  <label>Mall<select value={selected} onchange={e=>choose(e.currentTarget.value)} {disabled}>{#each choices as t}<option value={t.id}>{t.label}</option>{/each}</select></label>
  <details><summary>Egna mallar</summary><div class="editor">
    <label>Mallnamn<input bind:value={name} maxlength="80" {disabled} /></label>
    <label>Vad ska mallen göra?<textarea bind:value={instructions} maxlength={scope==='meeting'?12000:2000} rows="3" {disabled} placeholder="Beskriv önskad struktur och ton. Källans fakta ska behållas."></textarea></label>
    <button onclick={save} disabled={disabled||!name.trim()||!instructions.trim()||custom.length>=30}>Spara mall på datorn</button>
    {#if custom.some(t=>t.id===selected)}<button onclick={remove} {disabled}>Ta bort vald mall</button>{/if}
  </div></details>
  {#if error}<p role="alert">{error}</p>{/if}
</div>
<style>
  .templates{display:flex;align-items:start;gap:14px;flex-wrap:wrap;font:14px/1.5 Archivo,sans-serif}.templates>label{min-width:210px}label{display:grid;gap:6px;color:var(--muted)}select,input,textarea,button{box-sizing:border-box;font:inherit;color:var(--ink);background:var(--bg);border:1px solid var(--line-2);border-radius:6px;padding:9px;width:100%}button,summary{cursor:pointer}details{padding-top:26px;max-width:480px}summary{color:var(--accent)}.editor{display:grid;gap:10px;margin-top:12px}button:disabled{opacity:.5}:is(button,input,textarea,select,summary):focus-visible{outline:2px solid var(--accent);outline-offset:3px}p{color:#923115}
</style>
