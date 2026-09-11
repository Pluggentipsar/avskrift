<script lang="ts">
  import {activeWork,cancelActiveWork} from './work';
  let {dialog=false}:{dialog?:boolean}=$props();
  const labels:Record<string,string>={anonymize:'Avidentifierar transkript',analyze_document:'Avidentifierar underlaget',transcribe:'Transkriberar ljudfil',summarize:'Skapar sammanfattning',ask_transcript:'Bearbetar underlaget',create_grounded_draft:'Skapar källutkast',rewrite_dictation:'Bearbetar diktat'};
</script>
{#if $activeWork && (dialog || $activeWork.command!=='rewrite_dictation')}
  <div class="work-control" aria-label="Pågående arbete">
    <div role="status"><strong>{$activeWork.cancelling?'Avbryter…':$activeWork.finished?'Arbetet hann bli klart':labels[$activeWork.command]??'Arbetar lokalt'}</strong><p>{$activeWork.cancelling?'Väntar tills motorn kan stanna säkert. Tidigare resultat behålls.':'Du kan avbryta och behålla tidigare resultat.'}</p></div>
    <button onclick={cancelActiveWork} disabled={$activeWork.cancelling||$activeWork.finished}>Avbryt arbete</button>
    {#if $activeWork.error}<p role="alert">{$activeWork.error}</p>{/if}
  </div>
{/if}
<style>
  .work-control{display:flex;align-items:center;justify-content:space-between;gap:16px;flex-wrap:wrap;padding:16px 20px;margin:16px 0;border:1px solid var(--line);border-left:3px solid var(--accent);border-radius:8px;background:var(--bg);color:var(--ink);font:14px/1.5 Archivo,sans-serif}.work-control p{margin:4px 0 0;color:var(--muted)}button{flex-shrink:0;padding:9px 14px;border:1px solid var(--line-2);border-radius:6px;background:var(--bg);color:var(--ink);font:inherit;cursor:pointer}button:disabled{opacity:.5;cursor:default}button:focus-visible{outline:2px solid var(--accent);outline-offset:3px}
</style>
