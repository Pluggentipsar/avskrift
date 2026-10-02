<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { invokeWork, isWorkCancelled } from '$lib/work';
  import { paragraphs, playable, toEdited, fmt, tokenAt, type Project, type Preview, type Token, type EditList } from './types';

  let { project: initial, visible, onclose, onmodels }: {
    project: Project; visible: boolean; onclose: () => void; onmodels: () => void;
  } = $props();

  // The editor is keyed by project id, so it starts from the given project and then owns its state.
  const init = untrack(() => initial);
  let project = $state<Project>(init);
  const doc = $derived(paragraphs(project));
  const tokens = $derived(doc.flatMap(p => p.tokens).sort((a, b) => a.start - b.start));
  const soundIds = $derived(project.sounds.map(s => s.id));

  // --- edits, undo/redo -----------------------------------------------------------------------
  let deleted = $state(new Set<number>(init.edits.deleted));
  let pauseLimit = $state<number | null>(init.edits.pauseLimit);
  const removed = init.edits.removed;
  const undo: string[] = [], redo: string[] = [];
  let history = $state(0); // bumps so undo/redo buttons re-evaluate
  const snapshot = () => JSON.stringify({ d: [...deleted], p: pauseLimit });
  function restore(s: string) { const v = JSON.parse(s); deleted = new Set(v.d); pauseLimit = v.p; }
  function change(apply: () => void) {
    undo.push(snapshot()); redo.length = 0; apply(); history++; schedule();
  }
  function doUndo() { const s = undo.pop(); if (!s) return; redo.push(snapshot()); restore(s); history++; schedule(); }
  function doRedo() { const s = redo.pop(); if (!s) return; undo.push(snapshot()); restore(s); history++; schedule(); }
  // Plain data (no reactive proxies) for IPC.
  const edits = (): EditList => ($state.snapshot({ deleted: [...deleted].sort((a, b) => a - b), removed, pauseLimit }) as EditList);

  // --- preview (same cut points as export) and saving ---------------------------------------
  let preview = $state<Preview>({ keep: [[0, init.media.duration]], editedDuration: init.media.duration });
  let saveState = $state<'saved' | 'pending' | 'saving' | 'error'>('saved'), saveError = $state('');
  let previewTimer: ReturnType<typeof setTimeout> | undefined, saveTimer: ReturnType<typeof setTimeout> | undefined;
  let previewSeq = 0;
  function schedule() {
    saveState = 'pending';
    clearTimeout(previewTimer); clearTimeout(saveTimer);
    previewTimer = setTimeout(refreshPreview, 120);
    saveTimer = setTimeout(save, 700);
  }
  async function refreshPreview() {
    const seq = ++previewSeq;
    try { const p = await invoke<Preview>('textklipp_preview', { id: project.id, edits: edits() }); if (seq === previewSeq) preview = p; }
    catch (e) { saveError = `Förhandsvisningen kunde inte räknas fram: ${e}`; }
  }
  async function save() {
    saveState = 'saving';
    try { await invoke('textklipp_save_edits', { id: project.id, edits: edits() }); saveState = 'saved'; saveError = ''; }
    catch (e) { saveState = 'error'; saveError = String(e); }
  }
  export async function flush() { if (saveState === 'pending') { clearTimeout(saveTimer); await save(); } }

  // --- playback -------------------------------------------------------------------------------
  let video = $state<HTMLVideoElement>();
  let src = $state(''), isVideo = $state(false), mediaError = $state('');
  let time = $state(0), playing = $state(false), skipCuts = $state(true), showDeleted = $state(true);
  let currentId = $state(-1);
  async function loadMedia() {
    const m = await invoke<{ playback: string | null; isVideo: boolean }>('textklipp_media', { id: project.id });
    src = m.playback ? convertFileSrc(m.playback) : ''; isVideo = m.isVideo;
  }
  function frame() {
    if (!video) return;
    let t = video.currentTime;
    if (skipCuts && playing) {
      const next = playable(preview.keep, t);
      if (next === null) { video.pause(); }
      else if (next - t > 0.005) { video.currentTime = next; t = next; }
    }
    time = t;
    const i = tokenAt(tokens, t);
    currentId = i >= 0 && t <= tokens[i].end + 0.25 ? tokens[i].id : -1;
    if (playing) requestAnimationFrame(frame);
  }
  function togglePlay() {
    if (!video) return;
    if (video.paused) {
      if (skipCuts) { const next = playable(preview.keep, video.currentTime); video.currentTime = next ?? preview.keep[0]?.[0] ?? 0; }
      void video.play();
    } else video.pause();
  }
  function seek(t: number) { if (video) { video.currentTime = Math.max(0, t); time = video.currentTime; frame(); } }
  $effect(() => { if (!visible && video && !video.paused) video.pause(); });

  // --- selection → strike / restore -------------------------------------------------------------
  let docEl = $state<HTMLElement>();
  function selectedIds(): number[] {
    const sel = window.getSelection();
    if (!docEl || !sel || sel.rangeCount === 0 || sel.isCollapsed) return [];
    const ids: number[] = [];
    for (const el of docEl.querySelectorAll<HTMLElement>('[data-id]')) if (sel.containsNode(el, true)) ids.push(Number(el.dataset.id));
    return ids;
  }
  function strike(ids: number[]) {
    if (!ids.length) return;
    const restoring = ids.every(id => deleted.has(id));
    change(() => { const next = new Set(deleted); for (const id of ids) restoring ? next.delete(id) : next.add(id); deleted = next; });
    window.getSelection()?.removeAllRanges();
  }
  function onkey(e: KeyboardEvent) {
    if (!visible || (e.target as HTMLElement)?.closest('input,select,textarea')) return;
    const mod = e.ctrlKey || e.metaKey;
    if ((e.key === 'Delete' || e.key === 'Backspace') && !mod) { const ids = selectedIds(); if (ids.length) { e.preventDefault(); strike(ids); } }
    else if (mod && e.key.toLowerCase() === 'z' && !e.shiftKey) { e.preventDefault(); doUndo(); }
    else if (mod && (e.key.toLowerCase() === 'y' || (e.key.toLowerCase() === 'z' && e.shiftKey))) { e.preventDefault(); doRedo(); }
    else if (e.key === ' ' && !(e.target as HTMLElement)?.closest('button')) { e.preventDefault(); togglePlay(); }
  }
  function clickToken(t: Token, e: MouseEvent) {
    if (window.getSelection()?.isCollapsed === false) return; // a drag selection, not a click
    if (e.detail === 1) seek(t.start);
  }

  // --- tools ---------------------------------------------------------------------------------
  let proxyBusy = $state(false), proxyError = $state('');
  async function makeProxy() {
    proxyBusy = true; proxyError = '';
    try { project = await invokeWork<Project>('textklipp_make_proxy', { id: project.id }); await loadMedia(); }
    catch (e) { if (!isWorkCancelled(String(e))) proxyError = String(e); }
    finally { proxyBusy = false; }
  }
  const strikeSounds = () => change(() => { deleted = new Set([...deleted, ...soundIds]); });
  const restoreAll = () => change(() => { deleted = new Set(); pauseLimit = null; });
  function setPause(v: string) { change(() => { pauseLimit = v === '' ? null : Number(v); }); }
  const struckSounds = $derived(soundIds.filter(id => deleted.has(id)).length);

  // --- timeline ------------------------------------------------------------------------------
  const duration = $derived(project.media.duration || 1);
  function timelineClick(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    seek(((e.clientX - r.left) / r.width) * duration);
  }
  const gaps = $derived.by(() => {
    const out: [number, number][] = []; let t = 0;
    for (const [a, b] of preview.keep) { if (a > t) out.push([t, a]); t = b; }
    if (t < duration) out.push([t, duration]);
    return out;
  });
  /** Removed stretches, including a trimmed start or end – what the user thinks of as cuts. */
  const cuts = $derived(gaps.length);

  onMount(() => {
    void loadMedia(); void refreshPreview();
    window.addEventListener('keydown', onkey);
    return () => { window.removeEventListener('keydown', onkey); void flush(); };
  });
  $effect(() => { if (currentId >= 0 && playing && docEl) docEl.querySelector(`[data-id="${currentId}"]`)?.scrollIntoView({ block: 'nearest' }); });
</script>

<div class="editor">
  <header>
    <button class="link" onclick={async () => { await flush(); onclose(); }}>← Alla klipp</button>
    <h2>{project.title}</h2>
    <span class="save" class:error={saveState === 'error'} role="status">{saveState === 'saving' ? 'Sparar…' : saveState === 'pending' ? 'Ändringar väntar' : saveState === 'error' ? 'Kunde inte spara' : 'Sparat'}</span>
  </header>
  {#if saveError}<p class="banner error" role="alert">{saveError}</p>{/if}
  {#if project.wordTimes !== 'exakta'}
    <p class="banner warn">Ordtiderna kommer från talmodellen och kan ligga flera tiondels sekunder fel – klipp kan höras. Hämta <button class="link" onclick={onmodels}>Exakta ordtider</button> och importera filen igen.</p>
  {/if}

  <div class="layout">
    <section class="player" aria-label="Uppspelning">
      {#if src}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video bind:this={video} {src} class:audio={!isVideo} preload="auto"
          onplay={() => { playing = true; requestAnimationFrame(frame); }} onpause={() => { playing = false; frame(); }}
          onseeked={() => frame()} onerror={() => (mediaError = 'Uppspelningsfilen kunde inte spelas.')}></video>
      {:else}
        <div class="no-media"><p>{project.media.video ? 'Uppspelningskopian saknas.' : 'Ljudet saknas.'}</p>
          {#if project.media.video}<button class="btn" onclick={makeProxy} disabled={proxyBusy}>{proxyBusy ? 'Skapar kopia…' : 'Skapa uppspelningskopia'}</button>{/if}
          {#if proxyError || project.error}<p class="error">{proxyError || project.error}</p>{/if}</div>
      {/if}
      {#if mediaError}<p class="error">{mediaError}</p>{/if}
      <div class="controls">
        <button class="btn primary" onclick={togglePlay} disabled={!src} aria-label={playing ? 'Pausa' : 'Spela'}>{playing ? 'Pausa' : 'Spela'}</button>
        <span class="clock" title="Redigerad tid / total redigerad längd">{fmt(toEdited(preview.keep, time))} / {fmt(preview.editedDuration)}</span>
        <span class="source-clock" title="Tid i originalet">original {fmt(time)}</span>
        <label class="check"><input type="checkbox" bind:checked={skipCuts} /> Hoppa över borttaget</label>
      </div>
      <div class="timeline" role="slider" tabindex="0" aria-label="Tidslinje" aria-valuemin={0} aria-valuemax={Math.round(duration)} aria-valuenow={Math.round(time)}
        onclick={timelineClick} onkeydown={e => { if (e.key === 'ArrowRight') seek(time + 5); if (e.key === 'ArrowLeft') seek(time - 5); }}>
        {#each gaps as [a, b]}<span class="cut" style:left="{(a / duration) * 100}%" style:width="{Math.max(0.15, ((b - a) / duration) * 100)}%"></span>{/each}
        <span class="playhead" style:left="{(time / duration) * 100}%"></span>
      </div>
      <dl class="stats">
        <div><dt>Original</dt><dd>{fmt(duration)}</dd></div>
        <div><dt>Efter klipp</dt><dd>{fmt(preview.editedDuration)}</dd></div>
        <div><dt>Klipp</dt><dd>{cuts}</dd></div>
      </dl>
      <section class="tools" aria-label="Verktyg">
        <div class="row"><button class="btn" onclick={doUndo} disabled={history >= 0 && !undo.length}>Ångra</button><button class="btn" onclick={doRedo} disabled={history >= 0 && !redo.length}>Gör om</button>
          <button class="btn" onclick={restoreAll} disabled={!deleted.size && pauseLimit === null}>Återställ allt</button></div>
        <label>Korta pauser
          <select value={pauseLimit === null ? '' : String(pauseLimit)} onchange={e => setPause(e.currentTarget.value)}>
            <option value="">Behåll pauser som de är</option><option value="1.5">Längre än 1,5 s → 1,5 s</option>
            <option value="1">Längre än 1 s → 1 s</option><option value="0.7">Längre än 0,7 s → 0,7 s</option><option value="0.5">Längre än 0,5 s → 0,5 s</option>
          </select></label>
        {#if soundIds.length}<p>{soundIds.length} ljud utan ord i texten ({struckSounds} borttagna). <button class="link" onclick={strikeSounds} disabled={struckSounds === soundIds.length}>Ta bort alla</button></p>{/if}
        <label class="check"><input type="checkbox" bind:checked={showDeleted} /> Visa borttagen text</label>
        <p class="hint">Markera text och tryck <kbd>Delete</kbd> för att ta bort. Markera borttagen text och tryck <kbd>Delete</kbd> igen för att återställa. <kbd>Ctrl</kbd>+<kbd>Z</kbd> ångrar, mellanslag spelar och pausar. Klicka på ett ord för att hoppa dit.</p>
      </section>
    </section>

    <article class="doc" bind:this={docEl} aria-label="Transkript – markera text för att klippa">
      {#each doc as p (p.key)}
        <p class="para" class:hidden-deleted={!showDeleted && p.tokens.every(t => deleted.has(t.id))}>
          {#if p.speaker}<span class="speaker" contenteditable="false">{p.speaker}</span>{/if}
          {#each p.tokens as t (t.id)}{#if showDeleted || !deleted.has(t.id)}<span data-id={t.id} class:sound={t.kind === 'sound'} class:struck={deleted.has(t.id)} class:current={t.id === currentId}
            role="button" tabindex="-1" title={t.kind === 'sound' ? (t.text === 'ljud' ? 'Tal eller ljud som inte finns i texten' : `Finns inte i texten – modellen hörde "${t.text}"`) : fmt(t.start)}
            onclick={e => clickToken(t, e)} onkeydown={() => {}}>{t.kind === 'sound' ? `[${t.text}]` : t.text}</span>{' '}{/if}{/each}
        </p>
      {/each}
    </article>
  </div>
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 12px; min-height: 0; }
  header { display: flex; align-items: baseline; gap: 16px; }
  h2 { font: 28px 'Instrument Serif', serif; margin: 0; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .save { font-size: 12px; color: var(--muted); } .save.error, .error { color: #923115; }
  .banner { margin: 0; padding: 10px 12px; border-radius: 8px; background: var(--accent-soft); font-size: 13px; }
  .banner.warn { background: #fff4d6; color: #5c4400; } .banner.error { background: #fde8e4; }
  .layout { display: grid; grid-template-columns: minmax(320px, 5fr) 6fr; gap: 24px; min-height: 0; }
  .player { display: flex; flex-direction: column; gap: 12px; position: sticky; top: 0; align-self: start; }
  video { width: 100%; max-height: 52vh; background: #000; border-radius: 10px; } video.audio { height: 54px; background: transparent; }
  .no-media { aspect-ratio: 16/9; display: grid; place-content: center; gap: 8px; text-align: center; border: 1px dashed var(--line-2); border-radius: 10px; color: var(--muted); }
  .controls { display: flex; align-items: center; gap: 14px; flex-wrap: wrap; }
  .clock { font: 600 15px/1 Archivo, sans-serif; font-variant-numeric: tabular-nums; } .source-clock { font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .timeline { position: relative; height: 26px; border-radius: 6px; background: var(--accent-soft); cursor: pointer; overflow: hidden; }
  .timeline:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .cut { position: absolute; top: 0; bottom: 0; background: repeating-linear-gradient(135deg, #92311566 0 4px, #9231152a 4px 8px); }
  .playhead { position: absolute; top: -2px; bottom: -2px; width: 2px; background: var(--ink); transform: translateX(-1px); }
  .stats { display: flex; gap: 24px; margin: 0; } .stats div { display: grid; } dt { font-size: 11px; color: var(--muted); } dd { margin: 0; font-weight: 600; font-variant-numeric: tabular-nums; }
  .tools { display: grid; gap: 10px; border-top: 1px solid var(--line); padding-top: 12px; font-size: 13px; }
  .tools .row { display: flex; gap: 8px; flex-wrap: wrap; } .tools label:not(.check) { display: grid; gap: 4px; } .tools p { margin: 0; color: var(--muted); }
  .hint { font-size: 12px; } kbd { font: 11px Archivo, sans-serif; border: 1px solid var(--line-2); border-radius: 4px; padding: 0 4px; }
  .check { display: flex; align-items: center; gap: 6px; font-size: 13px; }
  select, .btn { font: inherit; color: var(--ink); background: var(--bg); border: 1px solid var(--line-2); border-radius: 6px; padding: 7px 10px; }
  .btn { cursor: pointer; } .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; } .btn:disabled { opacity: .5; cursor: default; }
  .link { font: inherit; background: none; border: 0; color: var(--accent); cursor: pointer; padding: 0; text-decoration: underline; }
  .doc { font: 17px/1.75 Archivo, sans-serif; max-height: calc(100dvh - 210px); overflow: auto; padding-right: 12px; user-select: text; }
  .para { margin: 0 0 14px; content-visibility: auto; contain-intrinsic-size: auto 90px; } .para.hidden-deleted { display: none; }
  .speaker { display: block; font-size: 12px; font-weight: 600; color: var(--muted); user-select: none; }
  .doc span[data-id] { cursor: text; border-radius: 3px; }
  .doc span[data-id]:hover { background: var(--accent-soft); }
  .struck { text-decoration: line-through; color: var(--muted); background: #9231151a; }
  .sound { color: #6a4a00; font-style: italic; font-size: 14px; }
  .current { background: var(--accent); color: #fff; } .current.struck { background: #923115; }
  @media (max-width: 900px) { .layout { grid-template-columns: 1fr; } .player { position: static; } .doc { max-height: none; } }
</style>
