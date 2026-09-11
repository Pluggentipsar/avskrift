<script lang="ts">
  import {invokeWork,isWorkCancelled} from "$lib/work";
  import WorkControl from "$lib/WorkControl.svelte";
  import {onMount} from 'svelte';
  import {invoke} from '@tauri-apps/api/core';
  import TemplatePicker from './TemplatePicker.svelte';
  let {text,original,model,onaccept,onclose}: {text:string;original:string;model:string;onaccept:(text:string)=>Promise<boolean>;onclose:()=>void}=$props();
  let dialog:HTMLDialogElement;
  let result=$state(''),generated=$state(''),instructions=$state(''),busy=$state(false),error=$state(''),approved=$state(false);
  onMount(()=>dialog.showModal());
  async function generate(){if(busy)return;busy=true;error='';try{const proposed=await invokeWork<string>('rewrite_dictation',{args:{text,model,instructions}});if(!proposed.trim())throw Error('Modellen gav ingen text. Försök igen.');result=generated=proposed;approved=false;}catch(e){error=String(e);}finally{busy=false;}}
  async function accept(){if(!approved||!result.trim())return;busy=true;error='';try{if(await onaccept(result))dialog.close();else error='Texten kunde inte användas. Diktatet är kvar.';}catch(e){error=String(e);}finally{busy=false;}}
</script>
<dialog bind:this={dialog} aria-labelledby="rewrite-title" onclose={onclose} oncancel={e=>{if(busy)e.preventDefault();}}>
  <div class="head"><div><h2 id="rewrite-title">Bearbeta diktat</h2><p>Jämför med underlaget och använd förslaget när du är nöjd.</p></div><button onclick={()=>dialog.close()} disabled={busy} aria-label="Stäng utan att använda förslaget">×</button></div>
  <TemplatePicker scope="dictation" disabled={busy} onchange={value=>instructions=value} />
  <button class="primary generate" onclick={generate} disabled={busy||!instructions}>{busy?'Arbetar lokalt…':result?'Skapa nytt förslag':'Skapa förslag'}</button>
  <WorkControl dialog />
  {#if error}<p role={isWorkCancelled(error)?"status":"alert"} class:error={!isWorkCancelled(error)}>{error}</p>{/if}
  <div class="columns"><section><h3>Underlag</h3><textarea readonly aria-label="Diktatets underlag" value={text}></textarea>{#if original!==text}<details><summary>Visa första originalet</summary><p class="original">{original}</p></details>{/if}</section>
    <section><h3>{result&&result!==generated?'Redigerat av dig':'AI-förslag'}</h3><textarea aria-label="Bearbetat diktat" bind:value={result} oninput={()=>approved=false} disabled={busy||!generated} placeholder="Förslaget visas här. Ditt diktat ändras när du godkänner och använder det."></textarea></section></div>
  <div class="foot"><label><input type="checkbox" bind:checked={approved} disabled={busy||!result.trim()} /> Jag har jämfört med underlaget</label><button class="primary" onclick={accept} disabled={busy||!approved||!result.trim()}>Godkänn och använd texten</button><button onclick={()=>dialog.close()} disabled={busy}>Stäng utan att använda</button></div>
  <p class="hint">Diktatets val för sparande gäller även den nya texten. Första originalet behålls. Inget skickas till andra program härifrån.</p>
</dialog>
<style>
  dialog{box-sizing:border-box;width:min(1100px,calc(100vw - 32px));max-height:calc(100dvh - 32px);padding:28px;border:1px solid var(--line);border-radius:12px;color:var(--ink);background:var(--bg);font:14px/1.6 Archivo,sans-serif}dialog::backdrop{background:#17172b66}.head,.foot{display:flex;gap:14px;justify-content:space-between;align-items:center;flex-wrap:wrap}h2{font:32px 'Instrument Serif',serif;margin:0}h3{font-size:15px}.head p,.hint{color:var(--muted)}button{font:inherit;border:1px solid var(--line-2);background:var(--bg);color:var(--ink);border-radius:6px;padding:9px 14px;cursor:pointer}.primary{background:var(--accent);color:white;border-color:var(--accent)}button:disabled{opacity:.5}.generate{margin:18px 0}.columns{display:grid;grid-template-columns:1fr 1fr;gap:24px}textarea{box-sizing:border-box;width:100%;height:320px;resize:vertical;padding:18px;font:16px/1.8 Archivo,sans-serif;color:var(--ink);background:var(--bg);border:1px solid var(--line);border-radius:6px}.foot{margin-top:24px}.error{color:#923115}.original{white-space:pre-wrap}:is(button,input,textarea,summary):focus-visible{outline:2px solid var(--accent);outline-offset:3px}@media(max-width:700px){.columns{grid-template-columns:1fr}dialog{padding:18px}textarea{height:220px}}
</style>
