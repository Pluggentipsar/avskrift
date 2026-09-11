<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  let { jobId, original, onrestore, onclose }: {jobId:string; original:string; onrestore:(version:string)=>Promise<boolean>; onclose:()=>void}=$props();
  let dialog:HTMLDialogElement;
  let versions=$state<{id:string;updatedAt:string;title:string}[]>([]);
  let selected=$state('original');
  let preview=$state('');
  let error=$state('');
  let pending=$state(false);
  let request=0;
  onMount(()=>{dialog.showModal(); preview=original; void reload();});
  async function reload(){try{versions=await invoke('list_job_versions',{id:jobId});}catch(e){error=String(e);}}
  async function select(id:string){
    const token=++request; selected=id;error='';preview='';pending=true;
    try{
      if(id==='original') preview=original;
      else {
        const j=await invoke<any>('open_job_version',{id:jobId,version:id});
        if(token===request) preview=[j.sourceText && 'Källtext\n'+j.sourceText,j.transcript && 'Transkript\n'+j.transcript.utterances?.map((u:any)=>u.text).join('\n'),j.summaryDraft && 'Sammanfattning\n'+j.summaryDraft,j.notes && 'Anteckningar\n'+j.notes].filter(Boolean).join('\n\n');
      }
    }catch(e){if(token===request)error=String(e);}finally{if(token===request)pending=false;}
  }
  async function restore(){pending=true;try{if(await onrestore(selected))dialog.close();}catch(e){error=String(e);}finally{pending=false;}}
</script>
<dialog bind:this={dialog} aria-labelledby="versions-title" onclose={onclose} oncancel={e=>{if(pending)e.preventDefault();}}>
  <div class="head"><h2 id="versions-title">Original och versioner</h2><button disabled={pending} onclick={()=>dialog.close()} aria-label="Stäng versioner">×</button></div>
  <p>Originalet behålls. De senaste 30 versionskopiorna och en eventuell kopia från äldre projektformat finns här. Autosparandet skapar högst en versionskopia per minut; före bearbetning och återställning sparas en extra.</p>
  {#if error}<p role="alert">{error}</p>{/if}
  <div class="content"><nav aria-label="Sparade versioner">
    <button aria-pressed={selected==='original'} onclick={()=>select('original')} disabled={pending}>Originalunderlag</button>
    {#each versions as version}<button aria-pressed={selected===version.id} disabled={pending} onclick={()=>select(version.id)}>{version.id==='legacy'?'Äldre projektformat':new Date(version.updatedAt).toLocaleString('sv-SE')}</button>{/each}
    {#if !versions.length}<p>Versionskopior visas när arbetet har sparats och ändrats.</p>{/if}
  </nav><label>Text i vald version<textarea readonly value={pending?'Hämtar version…':preview || 'Inget originalunderlag sparat ännu.'}></textarea></label></div>
  <div class="foot"><span>Vid återställning sparas först din nuvarande arbetskopia som en version.</span><button class="primary" disabled={pending || selected==='original' || !!error} onclick={restore}>Återställ denna version</button></div>
</dialog>
<style>
  dialog{box-sizing:border-box;width:min(920px,calc(100vw - 32px));max-height:calc(100dvh - 32px);padding:28px;border:1px solid var(--line);border-radius:12px;background:var(--bg);color:var(--ink);font:14px/1.6 Archivo,sans-serif}dialog::backdrop{background:#17172b66}.head,.foot{display:flex;align-items:center;justify-content:space-between;gap:18px}h2{font:32px 'Instrument Serif',serif;margin:0}p,.foot span{color:var(--muted)}button{font:inherit;padding:9px 12px;cursor:pointer;border:1px solid var(--line-2);border-radius:7px;background:var(--bg);color:var(--ink)}button[aria-pressed=true]{background:var(--accent-soft);color:var(--accent)}button:disabled{opacity:.5;cursor:default}.content{display:grid;grid-template-columns:210px minmax(0,1fr);gap:22px;margin:24px 0}nav{display:flex;flex-direction:column;gap:8px;max-height:380px;overflow:auto}nav button{text-align:left}.content label{display:grid;gap:8px}textarea{box-sizing:border-box;width:100%;height:350px;resize:vertical;padding:16px;font:15px/1.7 Archivo,sans-serif;border:1px solid var(--line-2);border-radius:7px;color:var(--ink);background:var(--bg)}.primary{background:var(--accent);color:white}.foot{flex-wrap:wrap}@media(max-width:600px){.content{grid-template-columns:1fr}nav{max-height:170px}dialog{padding:18px}}
</style>
