<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { open, ask } from '@tauri-apps/plugin-dialog';
  import { invokeWork, isWorkCancelled } from '$lib/work';
  import Editor from './Editor.svelte';
  import { fmt, bytes, type Project, type ProjectMeta, type Probe } from './types';

  type Model = { id: string; label: string; downloaded: boolean };
  let { visible, models, speechModel, progress, percent, onmodels }: {
    visible: boolean; models: Model[]; speechModel: string; progress: string; percent: number; onmodels: () => void;
  } = $props();

  let projects = $state<ProjectMeta[]>([]);
  let open_ = $state<Project | null>(null);
  let picked = $state<{ path: string; probe: Probe } | null>(null);
  let diarize = $state(false);
  let busy = $state(false), error = $state(''), notice = $state('');
  let alignReady = $state<boolean | null>(null);

  const model = $derived(models.find(m => m.id === speechModel));
  const modelProblem = $derived(
    speechModel === 'pianissimo-sv' ? 'Pianissimo ger inga ordtider. Välj en KB-Whisper-modell för textklipp.'
    : !model?.downloaded ? 'Talmodellen behöver hämtas först.' : '');

  async function refresh() {
    try { projects = (await invoke<ProjectMeta[] | null>('textklipp_list')) ?? []; } catch (e) { error = String(e); }
    try { alignReady = (await invoke<{ ready: boolean }>('wordalign_status')).ready; } catch { alignReady = null; }
  }
  async function pick() {
    error = ''; notice = '';
    const path = await open({ multiple: false, filters: [
      { name: 'Video och ljud', extensions: ['mp4', 'mov', 'm4v', 'mkv', 'webm', 'avi', 'mts', 'm2ts', 'wmv', 'mp3', 'm4a', 'wav', 'flac', 'aac', 'ogg', 'opus'] }] });
    if (typeof path !== 'string') return;
    try { picked = { path, probe: await invoke<Probe>('textklipp_probe', { path }) }; }
    catch (e) { error = String(e); picked = null; }
  }
  async function start() {
    if (!picked) return;
    busy = true; error = '';
    try {
      const p = await invokeWork<Project>('textklipp_import', { args: { path: picked.path, model: speechModel, language: 'sv', diarize } });
      picked = null; await refresh();
      if (p.error) notice = p.error;
      open_ = p;
    } catch (e) {
      const msg = String(e);
      if (isWorkCancelled(msg)) notice = 'Importen avbröts.'; else error = msg;
      await refresh();
    } finally { busy = false; }
  }
  async function openProject(id: string) {
    error = '';
    try { open_ = await invoke<Project>('textklipp_open', { id }); } catch (e) { error = String(e); }
  }
  async function remove(p: ProjectMeta) {
    if (!(await ask(`Ta bort "${p.title}"? Klipplistan, uppspelningskopian och det utdragna ljudet raderas. Originalfilen påverkas inte.`, { title: 'Ta bort klipp', kind: 'warning', okLabel: 'Ta bort', cancelLabel: 'Avbryt' }))) return;
    try { await invoke('textklipp_delete', { id: p.id }); await refresh(); } catch (e) { error = String(e); }
  }
  onMount(refresh);
  $effect(() => { if (visible && !open_) void refresh(); });
</script>

{#if open_}
  {#key open_.id}<Editor project={open_} {visible} {progress} {percent} {onmodels} onclose={() => { open_ = null; void refresh(); }} />{/key}
{:else}
  <div class="textklipp">
    <h2 class="big-title">Textklipp</h2>
    <p class="intro">Klipp video genom att redigera texten. Det du stryker i transkriptet försvinner ur filmen. Originalfilen ändras aldrig.</p>
    {#if error}<p class="banner error" role="alert">{error}</p>{/if}
    {#if notice}<p class="banner" role="status">{notice}</p>{/if}
    {#if alignReady === false}<p class="banner warn">För klipp utan hörbara skarvar behövs <strong>Exakta ordtider</strong>. <button class="link" onclick={onmodels}>Hämta i Modeller på datorn</button></p>{/if}

    <section class="import">
      {#if !picked}
        <button class="btn primary" onclick={pick} disabled={busy}>Välj video eller ljudfil…</button>
      {:else}
        {@const m = picked.probe.media}
        <div class="picked">
          <strong title={picked.path}>{picked.path.split(/[\\/]/).pop()}</strong>
          <span>{fmt(m.duration)}{#if m.video} · {m.video.width}×{m.video.height} · {Math.round(m.video.fps * 100) / 100} bilder/s{m.video.variableRate ? ' (varierande)' : ''}{:else} · bara ljud{/if} · {bytes(m.sizeBytes)}</span>
          <span class="hint">Projektet behöver upp till {bytes(picked.probe.workingBytes)} extra diskutrymme (ljud och uppspelningskopia).</span>
        </div>
        <label class="check"><input type="checkbox" bind:checked={diarize} disabled={busy} /> Skilj på talare</label>
        <p class="hint">Talmodell: <strong>{model?.label ?? speechModel}</strong> · <button class="link" onclick={onmodels}>Byt</button>{#if modelProblem}<br /><span class="error">{modelProblem}</span>{/if}</p>
        {#if busy}<div class="progress" role="status"><span>{progress || 'Importerar…'}</span><progress value={percent} max="100"></progress></div>
        {:else}<div class="row"><button class="btn primary" onclick={start} disabled={!!modelProblem}>Transkribera och öppna</button><button class="btn" onclick={() => (picked = null)}>Välj en annan fil</button></div>{/if}
      {/if}
    </section>

    {#if projects.length}
      <section><h3>Dina klipp</h3>
        <ul class="list">
          {#each projects as p (p.id)}
            <li>
              <button class="open" onclick={() => openProject(p.id)} disabled={p.status !== 'ready'}>
                <span class="title">{p.title}</span>
                <span class="meta">{fmt(p.duration)} · {p.status === 'ready' ? (p.hasProxy ? 'klar' : 'klar, utan uppspelningskopia') : p.status === 'importing' ? 'importeras eller avbröts' : 'misslyckades'} · {new Date(p.updatedAt).toLocaleDateString('sv-SE')}</span>
              </button>
              <button class="link" onclick={() => remove(p)} disabled={busy}>Ta bort</button>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
  </div>
{/if}

<style>
  .textklipp { display: grid; gap: 18px; max-width: 820px; }
  .big-title { font: 40px 'Instrument Serif', serif; margin: 0; } .intro { color: var(--muted); margin: 0; }
  h3 { font-size: 15px; margin: 0 0 8px; }
  .banner { margin: 0; padding: 10px 12px; border-radius: 8px; background: var(--accent-soft); font-size: 13px; }
  .banner.warn { background: #fff4d6; color: #5c4400; } .banner.error, .error { color: #923115; } .banner.error { background: #fde8e4; }
  .import { display: grid; gap: 12px; padding: 18px; border: 1px solid var(--line); border-radius: 10px; }
  .picked { display: grid; gap: 2px; } .picked strong { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .picked span { font-size: 13px; color: var(--muted); }
  .hint { font-size: 12px; color: var(--muted); margin: 0; }
  .row { display: flex; gap: 8px; flex-wrap: wrap; }
  .check { display: flex; align-items: center; gap: 6px; font-size: 13px; }
  .progress { display: grid; gap: 6px; font-size: 13px; } progress { width: 100%; accent-color: var(--accent); }
  .btn { font: inherit; color: var(--ink); background: var(--bg); border: 1px solid var(--line-2); border-radius: 6px; padding: 9px 14px; cursor: pointer; justify-self: start; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; } .btn:disabled { opacity: .5; cursor: default; }
  .link { font: inherit; background: none; border: 0; color: var(--accent); cursor: pointer; padding: 0; text-decoration: underline; }
  .list { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; }
  .list li { display: flex; align-items: center; gap: 12px; border: 1px solid var(--line); border-radius: 8px; padding: 4px 12px 4px 4px; }
  .open { flex: 1; display: grid; text-align: left; font: inherit; background: none; border: 0; padding: 8px; border-radius: 6px; cursor: pointer; color: var(--ink); min-width: 0; }
  .open:hover:not(:disabled) { background: var(--accent-soft); } .open:disabled { cursor: default; opacity: .7; }
  .title { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; } .meta { font-size: 12px; color: var(--muted); }
</style>
