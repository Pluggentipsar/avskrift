<script lang="ts">
  import {invoke} from '@tauri-apps/api/core';
  import type {Snippet} from 'svelte';
  let {children,revision,rejected,stale,approved,onapprove}: {children:Snippet;revision:string;rejected:number[];stale:boolean;approved:boolean;onapprove:()=>void}=$props();
  let result=$state(''),error=$state(''),loading=$state(false),request=0;
  $effect(()=>{
    const key=revision,ids=[...rejected],token=++request;loading=true;error='';result='';
    void invoke<string>('copy_anonymized',{rejected:ids}).then(text=>{if(token===request)result=text;}).catch(e=>{if(token===request)error=String(e);}).finally(()=>{if(token===request)loading=false;});
    return ()=>{request++;};
  });
</script>
<div class="comparison">
  <section><h3>Original med markeringar</h3>{@render children()}</section>
  <section><h3>Maskerad text</h3><p class="hint">Så här ser texten ut med dina aktuella val.</p>
    {#if error}<p role="alert">{error}</p>{/if}
    <textarea aria-label="Maskerad text – förhandsvisning" readonly value={loading?'Uppdaterar…':result}></textarea>
    <button onclick={onapprove} disabled={stale||loading||!!error||approved}>{approved?'Granskad av dig':'Markera texten som granskad'}</button>
    <p class="hint">Granskningen gäller dessa maskeringar och denna textversion. Ändrade val behöver granskas igen.</p>
  </section>
</div>
<style>
  .comparison{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:24px;margin-top:20px}section{min-width:0}h3{font-size:15px;margin:0 0 12px}textarea{box-sizing:border-box;width:100%;min-height:330px;max-height:70vh;resize:vertical;padding:20px;font:16px/1.8 Archivo,sans-serif;color:var(--ink);background:var(--bg);border:1px solid var(--line);border-radius:7px}.hint{color:var(--muted);font:13px/1.6 Archivo,sans-serif}.comparison button{font:14px Archivo,sans-serif;padding:10px 14px;border:1px solid var(--accent);border-radius:6px;color:var(--accent);background:var(--bg);cursor:pointer}.comparison button:disabled{opacity:.55}button:focus-visible,textarea:focus-visible{outline:2px solid var(--accent);outline-offset:3px}@media(max-width:1100px){.comparison{grid-template-columns:1fr}}
</style>
