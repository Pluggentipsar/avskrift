<script lang="ts">
  import { onMount } from 'svelte';
  import { save } from '@tauri-apps/plugin-dialog';
  import { revealItemInDir } from '@tauri-apps/plugin-opener';
  import { invokeWork, cancelActiveWork, isWorkCancelled } from '$lib/work';
  import { fmt } from './types';

  type Result = { output: string; extra: string[]; expectedDuration: number; videoDuration: number | null; audioDuration: number; syncOk: boolean; encoder: string | null; pieces: number; seconds: number };
  let { projectId, title, isVideo, editedDuration, cuts, progress, percent, before, onclose }: {
    projectId: string; title: string; isVideo: boolean; editedDuration: number; cuts: number;
    progress: string; percent: number; before: () => Promise<void>; onclose: () => void;
  } = $props();

  let dialog: HTMLDialogElement;
  let quality = $state<'high' | 'small'>('high');
  let srt = $state(false), vtt = $state(false), text = $state(false);
  let busy = $state(false), cancelling = $state(false), error = $state(''), result = $state<Result | null>(null);
  const ext = $derived(isVideo ? 'mp4' : 'm4a');
  onMount(() => dialog.showModal());

  async function run() {
    error = ''; result = null;
    await before();
    const output = await save({
      title: 'Spara klippt film', defaultPath: `${title} (klippt).${ext}`,
      filters: [{ name: isVideo ? 'MP4-video' : 'M4A-ljud', extensions: [ext] }],
    });
    if (!output) return;
    busy = true;
    try { result = await invokeWork<Result>('textklipp_export', { id: projectId, args: { output, quality, srt, vtt, text } }); }
    catch (e) { const m = String(e); error = isWorkCancelled(m) ? 'Exporten avbröts. Ingen fil sparades.' : m; }
    finally { busy = false; cancelling = false; }
  }
  async function cancel() { cancelling = true; await cancelActiveWork(); }
  const name = (p: string) => p.split(/[\\/]/).pop();
</script>

<dialog bind:this={dialog} aria-labelledby="tk-export-title" onclose={onclose} oncancel={e => { if (busy) e.preventDefault(); }}>
  <header><h2 id="tk-export-title">Exportera klippt {isVideo ? 'film' : 'ljud'}</h2>
    <button aria-label="Stäng" onclick={() => dialog.close()} disabled={busy}>×</button></header>
  <p class="summary">{fmt(editedDuration)}{cuts ? ` efter ${cuts} klipp` : ', inga klipp'}. Renderas från originalfilen i full upplösning; originalet ändras inte.</p>

  {#if result}
    <div class="done" role="status">
      <p><strong>Klar:</strong> {name(result.output)}</p>
      <p class={result.syncOk ? 'ok' : 'warn'}>{result.syncOk
        ? `Bild och ljud kontrollerade: lika långa (${fmt(result.audioDuration)}).`
        : `Varning: bild ${result.videoDuration?.toFixed(2) ?? '–'} s, ljud ${result.audioDuration.toFixed(2)} s – kontrollera filmen.`}</p>
      {#if result.extra.length}<p>Även: {result.extra.map(name).join(', ')}</p>{/if}
      <p class="hint">{result.pieces} delar på {Math.round(result.seconds)} s{result.encoder ? ` · ${result.encoder.replace('h264_', '').toUpperCase()}` : ''}</p>
      <div class="row"><button class="btn primary" onclick={() => revealItemInDir(result!.output)}>Visa i mappen</button><button class="btn" onclick={() => dialog.close()}>Stäng</button></div>
    </div>
  {:else}
    <fieldset disabled={busy}>
      <legend>Kvalitet</legend>
      <label class="radio"><input type="radio" bind:group={quality} value="high" /> Hög kvalitet <span class="hint">– nära originalet</span></label>
      <label class="radio"><input type="radio" bind:group={quality} value="small" /> Mindre fil <span class="hint">– ungefär en tredjedel så stor</span></label>
    </fieldset>
    <fieldset disabled={busy}>
      <legend>Följer med</legend>
      <label class="radio"><input type="checkbox" bind:checked={srt} /> Undertexter (.srt)</label>
      <label class="radio"><input type="checkbox" bind:checked={vtt} /> Undertexter (.vtt)</label>
      <label class="radio"><input type="checkbox" bind:checked={text} /> Den klippta texten (.txt)</label>
      <p class="hint">Sparas bredvid filmen med samma namn. Tiderna gäller den klippta filmen.</p>
    </fieldset>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if busy}
      <div class="progress" role="status"><span>{cancelling ? 'Avbryter…' : progress || 'Exporterar…'}</span><progress value={percent} max="100"></progress></div>
      <div class="row"><button class="btn" onclick={cancel} disabled={cancelling}>Avbryt</button></div>
    {:else}
      <div class="row"><button class="btn primary" onclick={run}>Välj plats och exportera…</button><button class="btn" onclick={() => dialog.close()}>Avbryt</button></div>
    {/if}
  {/if}
</dialog>

<style>
  dialog { box-sizing: border-box; width: min(560px, calc(100vw - 32px)); padding: 24px; border: 1px solid var(--line); border-radius: 12px; background: var(--bg); color: var(--ink); font: 14px/1.6 Archivo, sans-serif; }
  dialog::backdrop { background: #1c1d1a66; }
  header { display: flex; justify-content: space-between; align-items: start; gap: 12px; }
  h2 { font: 28px 'Instrument Serif', serif; margin: 0; }
  header button { font: inherit; font-size: 22px; background: none; border: 0; cursor: pointer; color: var(--muted); }
  .summary, .hint { color: var(--muted); } .hint { font-size: 12px; margin: 4px 0 0; }
  fieldset { border: 0; border-top: 1px solid var(--line); margin: 14px 0 0; padding: 12px 0 0; display: grid; gap: 6px; }
  legend { font-weight: 600; padding: 0; }
  .radio { display: flex; align-items: center; gap: 8px; }
  .row { display: flex; gap: 8px; margin-top: 16px; flex-wrap: wrap; }
  .btn { font: inherit; color: var(--ink); background: var(--bg); border: 1px solid var(--line-2); border-radius: 6px; padding: 9px 14px; cursor: pointer; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; } .btn:disabled { opacity: .5; cursor: default; }
  .progress { display: grid; gap: 6px; margin-top: 16px; } progress { width: 100%; accent-color: var(--accent); }
  .error, .warn { color: #923115; } .ok { color: var(--accent); }
  .done p { margin: 6px 0; }
</style>
