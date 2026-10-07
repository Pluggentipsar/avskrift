<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke, convertFileSrc } from '@tauri-apps/api/core';
  import { invokeWork, isWorkCancelled } from '$lib/work';
  import Detail from './Detail.svelte';
  import { ICONS } from '$lib/icons';
  import ExportDialog from './ExportDialog.svelte';
  import { paragraphs, playable, toEdited, toSource, fmt, fmtPrecise, tokenAt, search, parseTime, findAllRetakes, inKeep, subtract,
    type Project, type Preview, type Token, type EditList, type Edge, type Retake } from './types';

  let { project: initial, visible, progress = '', percent = 0, onclose, onmodels }: {
    project: Project; visible: boolean; progress?: string; percent?: number; onclose: () => void; onmodels: () => void;
  } = $props();
  let exporting = $state(false);

  // The editor is keyed by project id, so it starts from the given project and then owns its state.
  const init = untrack(() => initial);
  let project = $state<Project>(init);
  const doc = $derived(paragraphs(project));
  const tokens = $derived(doc.flatMap(p => p.tokens).sort((a, b) => a.start - b.start));
  const soundIds = $derived(project.sounds.map(s => s.id));

  // --- edits, undo/redo -----------------------------------------------------------------------
  let deleted = $state(new Set<number>(init.edits.deleted));
  let pauseLimit = $state<number | null>(init.edits.pauseLimit);
  // Manual timeline adjustments: extra removed ranges and forced kept ranges (dragged cut edges).
  let removed = $state<[number, number][]>(init.edits.removed ?? []);
  let kept = $state<[number, number][]>(init.edits.kept ?? []);
  // Split points from the scissors tool: they cut nothing, they divide the timeline into pieces.
  let splits = $state<number[]>(init.edits.splits ?? []);
  const undo: string[] = [], redo: string[] = [];
  let history = $state(0); // bumps so undo/redo buttons re-evaluate
  const snapshot = () => JSON.stringify({ d: [...deleted], p: pauseLimit, r: removed, k: kept, s: splits });
  function restore(s: string) { const v = JSON.parse(s); deleted = new Set(v.d); pauseLimit = v.p; removed = v.r ?? []; kept = v.k ?? []; splits = v.s ?? []; }
  function change(apply: () => void) {
    undo.push(snapshot()); redo.length = 0; apply(); history++; schedule();
  }
  function doUndo() { const s = undo.pop(); if (!s) return; redo.push(snapshot()); restore(s); history++; schedule(); }
  function doRedo() { const s = redo.pop(); if (!s) return; undo.push(snapshot()); restore(s); history++; schedule(); }
  // Plain data (no reactive proxies) for IPC.
  const edits = (): EditList => ($state.snapshot({ deleted: [...deleted].sort((a, b) => a - b), removed, pauseLimit, kept, splits }) as EditList);

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
  // What you hear lags the media clock by the audio output latency (large on Bluetooth). Only the
  // playhead and word highlight are shifted, and only while playing; cut times are never affected.
  const LATENCY_KEY = 'textklipp.latencyMs';
  let latencyMs = $state(0), latencyAuto = $state<number | null>(null), latencyManual = $state(false);
  function loadLatency() {
    try { const v = localStorage.getItem(LATENCY_KEY); if (v !== null) { latencyMs = Number(v) || 0; latencyManual = true; } } catch { /* storage unavailable */ }
    try {
      const ctx = new AudioContext();
      latencyAuto = Math.round(((ctx.outputLatency || 0) + (ctx.baseLatency || 0)) * 1000);
      void ctx.close();
      if (!latencyManual) latencyMs = latencyAuto;
    } catch { latencyAuto = null; }
  }
  function setLatency(v: number | null) {
    latencyManual = v !== null;
    latencyMs = v ?? latencyAuto ?? 0;
    try { if (v === null) localStorage.removeItem(LATENCY_KEY); else localStorage.setItem(LATENCY_KEY, String(v)); } catch { /* ignore */ }
  }
  /** Where the sound you hear is: the media time minus output latency while playing. */
  const heard = $derived(playing ? Math.max(0, time - latencyMs / 1000) : time);
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
    if (loop && playing && toEdited(preview.keep, t) >= loop.until) { video.currentTime = loop.from; t = loop.from; }
    time = t;
    const h = playing ? Math.max(0, t - latencyMs / 1000) : t;
    const i = tokenAt(tokens, h);
    currentId = i >= 0 && h <= tokens[i].end + 0.25 ? tokens[i].id : -1;
    if (playing) requestAnimationFrame(frame);
  }
  function togglePlay() {
    if (!video) return;
    if (video.paused) {
      if (skipCuts) { const next = playable(preview.keep, video.currentTime); video.currentTime = next ?? preview.keep[0]?.[0] ?? 0; }
      void video.play();
    } else video.pause();
  }
  function seek(t: number) { loop = null; if (video) { video.currentTime = Math.max(0, t); time = video.currentTime; frame(); } }
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
    if ((e.key === 'Delete' || e.key === 'Backspace') && !mod) {
      const ids = selectedIds();
      if (ids.length) { e.preventDefault(); strike(ids); }
      else if (markRange) { e.preventDefault(); removeMarked(); }
    }
    else if (!mod && (e.key === 'v' || e.key === 'V')) { e.preventDefault(); tool = 'select'; }
    else if (!mod && (e.key === 'c' || e.key === 'C')) { e.preventDefault(); tool = 'blade'; }
    else if (!mod && (e.key === 's' || e.key === 'S')) { e.preventDefault(); splitAt(heard); }
    else if (!mod && (e.key === 'i' || e.key === 'I')) { e.preventDefault(); setMark('in'); }
    else if (!mod && (e.key === 'o' || e.key === 'O')) { e.preventDefault(); setMark('out'); }
    else if (e.key === 'Escape') { if (markIn !== null || markOut !== null) markIn = markOut = null; else tool = 'select'; }
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
  const restoreAll = () => change(() => { deleted = new Set(); pauseLimit = null; removed = []; kept = []; splits = []; });
  function setPause(v: string) { change(() => { pauseLimit = v === '' ? null : Number(v); }); }
  const struckSounds = $derived(soundIds.filter(id => deleted.has(id)).length);

  // --- timeline ------------------------------------------------------------------------------
  const duration = $derived(project.media.duration || 1);
  function timelineClick(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    pick(((e.clientX - r.left) / r.width) * duration, 6 / r.width * duration);
  }
  const gaps = $derived.by(() => {
    const out: [number, number][] = []; let t = 0;
    for (const [a, b] of preview.keep) { if (a > t) out.push([t, a]); t = b; }
    if (t < duration) out.push([t, duration]);
    return out;
  });
  /** Removed stretches, including a trimmed start or end – what the user thinks of as cuts. */
  const cuts = $derived(gaps.length);
  const fps = $derived(project.media.video?.fps || 25);

  // --- cut edges: drag in the detail view or nudge by one frame ------------------------------
  /** Move a cut edge. Pulling a cut smaller keeps more (forced keep); pushing it larger removes more. */
  function moveEdge(edge: Edge, to: number) {
    to = Math.max(0, Math.min(duration, to));
    if (Math.abs(to - edge.at) < 1e-4) return;
    const grows = edge.kind === 'out' ? to < edge.at : to > edge.at; // the cut gets longer
    const range: [number, number] = [Math.min(edge.at, to), Math.max(edge.at, to)];
    change(() => {
      if (grows) { removed = [...removed, range]; kept = kept.filter(([a, b]) => b <= range[0] || a >= range[1]); }
      else { kept = [...kept, range]; removed = removed.filter(([a, b]) => b <= range[0] || a >= range[1]); }
    });
  }
  /** The cut at or next after the playhead (else the last one): what nudging and listening act on. */
  const activeCut = $derived(gaps.find(([, b]) => b >= time - 0.05) ?? gaps[gaps.length - 1] ?? null);
  function nudge(side: 'start' | 'end', frames: number) {
    if (!activeCut) return;
    const at = side === 'start' ? activeCut[0] : activeCut[1];
    moveEdge({ kind: side === 'start' ? 'out' : 'in', at }, at + frames / fps);
  }
  function gotoCut(dir: 1 | -1) {
    const list = dir > 0 ? gaps.filter(([a]) => a > time + 1.05) : gaps.filter(([a]) => a < time + 0.95).reverse();
    const g = list[0]; if (g) seek(Math.max(0, g[0] - 1));
  }

  // --- manual ranges: mark in/out at the playhead (I/O) or drag in the detail view ---------------
  const snap = (t: number) => Math.max(0, Math.min(duration, Math.round(t * fps) / fps));
  let markIn = $state<number | null>(null), markOut = $state<number | null>(null);
  const markRange = $derived<[number, number] | null>(markIn !== null && markOut !== null && Math.abs(markOut - markIn) >= 1 / fps
    ? [Math.min(markIn, markOut), Math.max(markIn, markOut)] : null);
  function setMark(side: 'in' | 'out') { const t = snap(heard); if (side === 'in') markIn = t; else markOut = t; }
  function setRange(a: number, b: number) { markIn = snap(Math.min(a, b)); markOut = snap(Math.max(a, b)); }
  const byId = $derived(new Map(tokens.map(t => [t.id, t])));
  /** Remove a stretch of time, whatever words lie in it. */
  function removeRange(a: number, b: number) {
    change(() => { removed = [...subtract(removed, a, b), [a, b]]; kept = subtract(kept, a, b); });
  }
  /** Bring a stretch back: manual removals in it go, words wholly inside are restored. */
  function keepRange(a: number, b: number) {
    change(() => {
      removed = subtract(removed, a, b);
      deleted = new Set([...deleted].filter(id => { const t = byId.get(id); return !t || t.start < a || t.end > b; }));
      kept = [...subtract(kept, a, b), [a, b]];
    });
  }
  function removeMarked() { if (markRange) { removeRange(...markRange); markIn = markOut = null; } }

  // --- scissors: split, then select a piece and remove it, as in a video editor -------------------
  let tool = $state<'select' | 'blade'>('select');
  function splitAt(t: number) {
    const at = snap(t);
    if (at <= 0 || at >= duration || splits.some(s => Math.abs(s - at) < 0.5 / fps)) return;
    change(() => { splits = [...splits, at].sort((a, b) => a - b); });
  }
  /** The piece around `t`, when at least one of its ends is a split (else clicking just moves the playhead). */
  function pieceAt(t: number): [number, number] | null {
    const bounds = [0, duration, ...splits, ...preview.keep.flat()].sort((a, b) => a - b);
    const lo = Math.max(...bounds.filter(b => b <= t)), hi = Math.min(...bounds.filter(b => b > t));
    const isSplit = (b: number) => splits.some(s => Math.abs(s - b) < 1e-3);
    return isFinite(lo) && isFinite(hi) && (isSplit(lo) || isSplit(hi)) ? [lo, hi] : null;
  }
  /** A click on the timeline or the waveform: split (or unsplit) with the scissors, else move the
   *  playhead and select the piece between splits. `near` is how close counts as hitting a split. */
  function pick(t: number, near: number) {
    if (tool === 'blade') {
      const hit = splits.find(s => Math.abs(s - t) <= near);
      if (hit !== undefined) change(() => { splits = splits.filter(s => s !== hit); });
      else splitAt(t);
      return;
    }
    seek(t);
    const piece = pieceAt(t);
    if (piece) setRange(...piece); else if (markRange) markIn = markOut = null;
  }
  function keepMarked() { if (markRange) { keepRange(...markRange); markIn = markOut = null; } }
  /** Words not struck but inside removed time (a cut silence or a manual range): shown as cut. */
  const cutAway = (t: Token) => !deleted.has(t.id) && !inKeep(preview.keep, (t.start + t.end) / 2);

  // --- long silences, found from the audio (words can lie across them) --------------------------
  const SILENCE_KEEP = 0.25; // seconds of each silence kept on both sides, so joins breathe
  let silenceMin = $state(2), silences = $state<[number, number][]>([]);
  async function loadSilences() {
    try { silences = (await invoke<[number, number][]>('textklipp_silences', { id: project.id, min: silenceMin })) ?? []; } catch { silences = []; }
  }
  const silenceCut = (s: [number, number]): [number, number] => [snap(s[0] + SILENCE_KEEP), snap(s[1] - SILENCE_KEEP)];
  const silenceGone = (s: [number, number]) => { const [a, b] = silenceCut(s); return !inKeep(preview.keep, (a + b) / 2); };
  function toggleSilence(s: [number, number]) { const [a, b] = silenceCut(s); if (silenceGone(s)) keepRange(a, b); else removeRange(a, b); }
  const silencesLeft = $derived(silences.filter(s => !silenceGone(s)));
  function removeSilences() {
    const ranges = silencesLeft.map(silenceCut);
    if (!ranges.length) return;
    change(() => { for (const [a, b] of ranges) { removed = [...subtract(removed, a, b), [a, b]]; kept = subtract(kept, a, b); } });
  }
  /** Silence markers in the text, before the first token that starts inside or after each silence. */
  const silenceBefore = $derived.by(() => {
    const at = new Map<number, [number, number][]>();
    for (const s of silences) {
      const i = tokens.findIndex(t => t.start >= s[0] - 0.05);
      const key = i >= 0 ? tokens[i].id : -1;
      at.set(key, [...(at.get(key) ?? []), s]);
    }
    return at;
  });
  const secs = (s: [number, number]) => Math.round(s[1] - s[0]);

  // --- listen to a join in a loop: 1 s of edited time before and after ------------------------
  let loop = $state<{ from: number; until: number } | null>(null);
  function listen() {
    if (!activeCut || !video) return;
    const join = toEdited(preview.keep, activeCut[0]);
    loop = { from: toSource(preview.keep, Math.max(0, join - 1)), until: join + 1 };
    skipCuts = true; video.currentTime = loop.from; void video.play();
  }
  function stopListen() { loop = null; video?.pause(); }

  // --- search and go to time ---------------------------------------------------------------
  let query = $state(''), hitIndex = $state(0), hitShown = false, timeInput = $state(''), timeError = $state(false);
  const hits = $derived(query.trim().length >= 2 ? search(tokens, query) : []);
  const hitIds = $derived(new Set(hits.flatMap(([a, b]) => tokens.slice(a, b + 1).map(t => t.id))));
  function showHit(i: number) {
    if (!hits.length) return;
    hitIndex = (i + hits.length) % hits.length;
    const tok = tokens[hits[hitIndex][0]];
    seek(tok.start);
    docEl?.querySelector(`[data-id="${tok.id}"]`)?.scrollIntoView({ block: 'center' });
  }
  function goTime() {
    const s = parseTime(timeInput);
    timeError = s === null || s > duration;
    if (!timeError) { seek(s!); const i = tokenAt(tokens, s!); if (i >= 0) docEl?.querySelector(`[data-id="${tokens[i].id}"]`)?.scrollIntoView({ block: 'center' }); }
  }

  // --- repeated takes ----------------------------------------------------------------------
  const retakes = $derived(findAllRetakes(tokens));
  /** Token id → the retake it starts (chip in the text) and the set of ids in earlier takes. */
  const retakeAt = $derived(new Map<number, Retake>(retakes.map(r => [tokens[r.from].id, r])));
  const retakeTokenIds = $derived(new Set(retakes.flatMap(r => tokens.slice(r.from, r.earlierTo + 1).map(t => t.id))));
  const retakeIds = (r: { from: number; to: number }) => tokens.slice(r.from, r.to + 1).map(t => t.id);
  const retakeDone = (r: { from: number; to: number }) => retakeIds(r).every(id => deleted.has(id));
  function removeRetake(r: { from: number; to: number }) { change(() => { deleted = new Set([...deleted, ...retakeIds(r)]); }); }
  function showRetake(r: { from: number; start: number }) { seek(r.start); docEl?.querySelector(`[data-id="${tokens[r.from].id}"]`)?.scrollIntoView({ block: 'center' }); }

  onMount(() => {
    void loadMedia(); void refreshPreview(); void loadSilences(); loadLatency();
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
    <button class="btn primary" onclick={() => { video?.pause(); exporting = true; }} disabled={!preview.editedDuration}>Exportera…</button>
  </header>
  {#if exporting}
    <ExportDialog projectId={project.id} title={project.title} isVideo={!!project.media.video} editedDuration={preview.editedDuration} {cuts}
      {progress} {percent} before={flush} onclose={() => (exporting = false)} />
  {/if}
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
      <div class="cuttools" role="toolbar" aria-label="Verktyg för tidslinjen">
        <div class="toolswitch" role="group" aria-label="Verktyg">
          <button aria-pressed={tool === 'select'} onclick={() => (tool = 'select')} title="Markera (V): klicka för att flytta markören eller välja en bit mellan två delningar, dra för att markera">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 3l14 8-6 1.5L10 19z" /></svg>Markera</button>
          <button aria-pressed={tool === 'blade'} onclick={() => (tool = 'blade')} title="Sax (C): klicka på vågformen eller tidslinjen för att dela där; klicka på en delning för att ta bort den">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICONS.textklipp} /></svg>Sax</button>
        </div>
        <button class="btn small" onclick={() => splitAt(heard)} title="Dela vid uppspelningsmarkören (S)">Dela vid markören <kbd>S</kbd></button>
        {#if splits.length}<span class="hint">{splits.length} {splits.length === 1 ? 'delning' : 'delningar'}</span>{/if}
      </div>
      <div class="timeline" class:blade={tool === 'blade'} role="slider" tabindex="0" aria-label="Tidslinje" aria-valuemin={0} aria-valuemax={Math.round(duration)} aria-valuenow={Math.round(time)}
        onclick={timelineClick} onkeydown={e => { if (e.key === 'ArrowRight') seek(time + 5); if (e.key === 'ArrowLeft') seek(time - 5); }}>
        {#each silencesLeft as [a, b]}<span class="quiet" style:left="{(a / duration) * 100}%" style:width="{Math.max(0.15, ((b - a) / duration) * 100)}%"></span>{/each}
        {#each gaps as [a, b]}<span class="cut" style:left="{(a / duration) * 100}%" style:width="{Math.max(0.15, ((b - a) / duration) * 100)}%"></span>{/each}
        {#each splits as s (s)}<span class="split" style:left="{(s / duration) * 100}%"></span>{/each}
        {#if markIn !== null}<span class="mark-edge" style:left="{(markIn / duration) * 100}%"></span>{/if}
        {#if markOut !== null}<span class="mark-edge" style:left="{(markOut / duration) * 100}%"></span>{/if}
        {#if markRange}<span class="mark" style:left="{(markRange[0] / duration) * 100}%" style:width="{Math.max(0.15, ((markRange[1] - markRange[0]) / duration) * 100)}%"></span>{/if}
        <span class="playhead" style:left="{(heard / duration) * 100}%"></span>
      </div>
      <Detail projectId={project.id} time={heard} {duration} keep={preview.keep} {tokens} {deleted} {fps} active={activeCut} mark={markRange} silences={silencesLeft}
        {tool} {splits} onpick={pick} onedge={moveEdge} onrange={setRange} />
      <div class="markbar" role="group" aria-label="Klipp ett eget tidsavsnitt">
        <button class="btn small" onclick={() => setMark('in')} title="Sätt början av avsnittet vid markören (tangent I)">Markera in <kbd>I</kbd></button>
        <button class="btn small" onclick={() => setMark('out')} title="Sätt slutet av avsnittet vid markören (tangent O)">Markera ut <kbd>O</kbd></button>
        {#if markRange}
          <span class="markinfo">{fmtPrecise(markRange[0])}–{fmtPrecise(markRange[1])} ({(markRange[1] - markRange[0]).toLocaleString('sv-SE', { maximumFractionDigits: 1 })} s)</span>
          <button class="btn small primary" onclick={removeMarked}>Ta bort markerat <kbd>Delete</kbd></button>
          <button class="btn small" onclick={keepMarked}>Behåll markerat</button>
          <button class="btn small" onclick={() => (markIn = markOut = null)} aria-label="Rensa markeringen">×</button>
        {:else if markIn !== null || markOut !== null}
          <span class="markinfo">{markIn !== null ? `In ${fmtPrecise(markIn)} – sätt ut med O` : `Ut ${fmtPrecise(markOut ?? 0)} – sätt in med I`}</span>
        {:else}<span class="hint">{tool === 'blade' ? 'Klicka där du vill dela. Byt sedan till Markera och klicka på biten.' : 'eller dra över vågformen, eller dela med saxen'}</span>{/if}
      </div>
      <div class="cutbar" role="group" aria-label="Klipp">
        <button class="btn" onclick={() => gotoCut(-1)} disabled={!gaps.length}>◀ Föregående klipp</button>
        <button class="btn" onclick={() => gotoCut(1)} disabled={!gaps.length}>Nästa klipp ▶</button>
        {#if loop}<button class="btn primary" onclick={stopListen}>Sluta lyssna</button>
        {:else}<button class="btn" onclick={listen} disabled={!activeCut || !src} title="Spelar 1 s före och efter skarven om och om igen">Lyssna på skarven</button>{/if}
      </div>
      {#if activeCut}
        <div class="nudge" role="group" aria-label="Finjustera klippet">
          <span>Klipp {fmtPrecise(activeCut[0])}–{fmtPrecise(activeCut[1])} ({(activeCut[1] - activeCut[0]).toLocaleString('sv-SE', { maximumFractionDigits: 2 })} s)</span>
          <span>början <button class="btn small" onclick={() => nudge('start', -1)} aria-label="Börja klippet en bildruta tidigare">−1</button><button class="btn small" onclick={() => nudge('start', 1)} aria-label="Börja klippet en bildruta senare">+1</button></span>
          <span>slut <button class="btn small" onclick={() => nudge('end', -1)} aria-label="Sluta klippet en bildruta tidigare">−1</button><button class="btn small" onclick={() => nudge('end', 1)} aria-label="Sluta klippet en bildruta senare">+1</button></span>
          <span class="hint">bildruta</span>
        </div>
      {/if}
      <dl class="stats">
        <div><dt>Original</dt><dd>{fmt(duration)}</dd></div>
        <div><dt>Efter klipp</dt><dd>{fmt(preview.editedDuration)}</dd></div>
        <div><dt>Klipp</dt><dd>{cuts}</dd></div>
      </dl>
      <section class="tools" aria-label="Verktyg">
        <div class="row"><button class="btn" onclick={doUndo} disabled={history >= 0 && !undo.length}>Ångra</button><button class="btn" onclick={doRedo} disabled={history >= 0 && !redo.length}>Gör om</button>
          <button class="btn" onclick={restoreAll} disabled={!deleted.size && pauseLimit === null && !removed.length && !kept.length && !splits.length}>Återställ allt</button></div>
        <label>Korta pauser
          <select value={pauseLimit === null ? '' : String(pauseLimit)} onchange={e => setPause(e.currentTarget.value)}>
            <option value="">Behåll pauser som de är</option><option value="1.5">Längre än 1,5 s → 1,5 s</option>
            <option value="1">Längre än 1 s → 1 s</option><option value="0.7">Längre än 0,7 s → 0,7 s</option><option value="0.5">Längre än 0,5 s → 0,5 s</option>
          </select></label>
        <div class="silences">
          <label>Långa tystnader
            <select value={String(silenceMin)} onchange={e => { silenceMin = Number(e.currentTarget.value); void loadSilences(); }}>
              <option value="1">Minst 1 s</option><option value="2">Minst 2 s</option><option value="3">Minst 3 s</option><option value="5">Minst 5 s</option><option value="10">Minst 10 s</option>
            </select></label>
          {#if silences.length}
            <p>{silences.length} tysta partier, {fmt(silences.reduce((n, q) => n + q[1] - q[0], 0))} sammanlagt ({silences.length - silencesLeft.length} borttagna).
              <button class="link" onclick={removeSilences} disabled={!silencesLeft.length}>Ta bort alla tystnader</button></p>
            <p class="hint">Hittas i ljudet, även där ord ligger utspridda över tystnaden. {SILENCE_KEEP.toLocaleString('sv-SE')} s behålls i varje kant. Markeras med ⏸ i texten.</p>
          {:else}<p class="hint">Inga tysta partier så långa.</p>{/if}
        </div>
        {#if soundIds.length}<p>{soundIds.length} ljud utan ord i texten ({struckSounds} borttagna). <button class="link" onclick={strikeSounds} disabled={struckSounds === soundIds.length}>Ta bort alla</button></p>{/if}
        {#if retakes.length}
          <details class="retakes" open={retakes.length <= 4}>
            <summary>{retakes.length} möjliga omtagningar</summary>
            <p class="hint">Sådant som sägs igen strax efter: hela meningar och omstarter mitt i en mening. Markeras med ↺ i texten. Ta bort den tidigare tagningen för att behålla den senare.</p>
            <ul>{#each retakes as r (r.from)}
              <li class:done={retakeDone(r)}>
                <button class="link" onclick={() => showRetake(r)}>{fmt(r.start)}</button> ”{r.earlier.length > 60 ? r.earlier.slice(0, 60) + '…' : r.earlier}” → {r.kind === 'restart' ? 'börjar om' : 'sägs igen'}
                <span class="hint">({Math.round(r.end - r.start)} s)</span>
                {#if retakeDone(r)}<span class="hint">borttagen</span>{:else}<button class="link" onclick={() => removeRetake(r)}>Ta bort tidigare tagning</button>{/if}
              </li>{/each}</ul>
          </details>
        {/if}
        <label class="check"><input type="checkbox" bind:checked={showDeleted} /> Visa borttagen text</label>
        <div class="latency">
          <label for="latency">Markörens synk mot ljudet: <strong>{latencyMs} ms</strong></label>
          <input id="latency" type="range" min="0" max="400" step="10" value={latencyMs} oninput={e => setLatency(Number(e.currentTarget.value))} />
          <span class="hint">{latencyManual ? 'Inställt för hand.' : latencyAuto !== null ? 'Uppmätt för din ljudenhet.' : 'Kunde inte mätas.'}
            {#if latencyManual}<button class="link" onclick={() => setLatency(null)}>Mät automatiskt{latencyAuto !== null ? ` (${latencyAuto} ms)` : ''}</button>{/if}
            Öka om markören ligger före det du hör, till exempel med Bluetooth-hörlurar. Påverkar inte klippen.</span>
        </div>
        <p class="hint">Markera text och tryck <kbd>Delete</kbd> för att ta bort. Markera borttagen text och tryck <kbd>Delete</kbd> igen för att återställa. <kbd>Ctrl</kbd>+<kbd>Z</kbd> ångrar, mellanslag spelar och pausar. Klicka på ett ord för att hoppa dit. Dra i en röd kant i detaljvyn för att flytta ett klipp. Klipp ett eget avsnitt: dela med saxen (<kbd>C</kbd>, eller <kbd>S</kbd> vid markören), välj biten med Markera (<kbd>V</kbd>) och tryck <kbd>Delete</kbd>. Det går också att dra över vågformen eller använda <kbd>I</kbd> och <kbd>O</kbd>.</p>
      </section>
    </section>

    <div class="textcol">
    <div class="find" role="search">
      <input type="search" placeholder="Sök i texten" aria-label="Sök i texten" bind:value={query} oninput={() => { hitIndex = 0; hitShown = false; }}
        onkeydown={e => { if (e.key === 'Enter') { e.preventDefault(); showHit(!hitShown ? 0 : hitIndex + (e.shiftKey ? -1 : 1)); hitShown = true; } if (e.key === 'Escape') query = ''; }} />
      {#if query.trim().length >= 2}<span class="hint" role="status">{hits.length ? `${hitIndex + 1} av ${hits.length}` : 'Inga träffar'}</span>
        <button class="btn small" onclick={() => showHit(hitIndex - 1)} disabled={!hits.length} aria-label="Föregående träff">↑</button>
        <button class="btn small" onclick={() => showHit(hitIndex + 1)} disabled={!hits.length} aria-label="Nästa träff">↓</button>{/if}
      <input class="time" placeholder="Gå till tid" aria-label="Gå till tid (minuter:sekunder)" bind:value={timeInput} class:invalid={timeError} oninput={() => (timeError = false)}
        onkeydown={e => { if (e.key === 'Enter') { e.preventDefault(); goTime(); } }} />
    </div>
    <article class="doc" bind:this={docEl} aria-label="Transkript – markera text för att klippa">
      {#each doc as p (p.key)}
        <p class="para" class:hidden-deleted={!showDeleted && p.tokens.every(t => deleted.has(t.id))}>
          {#if p.speaker}<span class="speaker" contenteditable="false">{p.speaker}</span>{/if}
          {#each p.tokens as t (t.id)}{#each silenceBefore.get(t.id) ?? [] as q (q[0])}<button class="pause-mark" class:gone={silenceGone(q)} contenteditable="false"
              title={silenceGone(q) ? 'Tystnaden är borttagen – klicka för att ta tillbaka den' : `Tyst i ${secs(q)} s – klicka för att ta bort`} onclick={() => toggleSilence(q)}>⏸ {secs(q)} s</button>{' '}{/each}{#if retakeAt.has(t.id)}{@const r = retakeAt.get(t.id)!}<button class="retake-mark" class:gone={retakeDone(r)} contenteditable="false"
              title={retakeDone(r) ? 'Den tidigare tagningen är borttagen' : `${r.kind === 'restart' ? 'Börjar om' : 'Sägs igen'}: ”${r.later}”. Klicka för att ta bort den tidigare tagningen.`}
              onclick={() => { if (!retakeDone(r)) removeRetake(r); }}>↺</button>{' '}{/if}{#if showDeleted || !deleted.has(t.id)}<span data-id={t.id} class:sound={t.kind === 'sound'} class:struck={deleted.has(t.id)} class:cutaway={cutAway(t)} class:retake={retakeTokenIds.has(t.id) && !deleted.has(t.id)} class:current={t.id === currentId} class:hit={hitIds.has(t.id)}
            role="button" tabindex="-1" title={t.kind === 'sound' ? (t.text === 'ljud' ? 'Tal eller ljud som inte finns i texten' : `Finns inte i texten – modellen hörde "${t.text}"`) : fmt(t.start)}
            onclick={e => clickToken(t, e)} onkeydown={() => {}}>{t.kind === 'sound' ? `[${t.text}]` : t.text}</span>{' '}{/if}{/each}
        </p>
      {/each}
      {#each silenceBefore.get(-1) ?? [] as q (q[0])}<p class="para"><button class="pause-mark" class:gone={silenceGone(q)} onclick={() => toggleSilence(q)}>⏸ {secs(q)} s</button></p>{/each}
    </article>
    </div>
  </div>
</div>

<style>
  .editor { display: flex; flex-direction: column; gap: 12px; min-height: 0; }
  header { display: flex; align-items: center; gap: 16px; }
  h2 { font: 28px 'Instrument Serif', serif; margin: 0; flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .save { font-size: 12px; color: var(--muted); } .save.error, .error { color: #923115; }
  .banner { margin: 0; padding: 10px 12px; border-radius: 8px; background: var(--accent-soft); font-size: 13px; }
  .banner.warn { background: #fff4d6; color: #5c4400; } .banner.error { background: #fde8e4; }
  .layout { display: grid; grid-template-columns: minmax(0, 5fr) minmax(0, 6fr); gap: 24px; min-height: 0; }
  .player { display: flex; flex-direction: column; gap: 12px; position: sticky; top: 0; align-self: start; min-width: 0; }
  video { width: 100%; max-height: 52vh; background: #000; border-radius: 10px; } video.audio { height: 54px; background: transparent; }
  .no-media { aspect-ratio: 16/9; display: grid; place-content: center; gap: 8px; text-align: center; border: 1px dashed var(--line-2); border-radius: 10px; color: var(--muted); }
  .controls { display: flex; align-items: center; gap: 14px; flex-wrap: wrap; }
  .clock { font: 600 15px/1 Archivo, sans-serif; font-variant-numeric: tabular-nums; } .source-clock { font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .timeline { position: relative; height: 26px; border-radius: 6px; background: var(--accent-soft); cursor: pointer; overflow: hidden; }
  .timeline:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .cut { position: absolute; top: 0; bottom: 0; background: repeating-linear-gradient(135deg, #92311566 0 4px, #9231152a 4px 8px); }
  .quiet { position: absolute; top: 0; bottom: 0; background: repeating-linear-gradient(90deg, #85570040 0 2px, transparent 2px 5px); }
  .mark { position: absolute; top: 0; bottom: 0; background: #1f4e4633; border-left: 2px solid var(--accent); border-right: 2px solid var(--accent); }
  .mark-edge { position: absolute; top: 0; bottom: 0; width: 2px; background: var(--accent); }
  .cuttools { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; font-size: 13px; }
  .toolswitch { display: inline-flex; border: 1px solid var(--line-2); border-radius: 7px; overflow: hidden; }
  .toolswitch button { display: inline-flex; align-items: center; gap: 6px; font: inherit; font-size: 13px; border: 0; background: var(--bg); color: var(--muted); padding: 6px 12px; cursor: pointer; }
  .toolswitch button + button { border-left: 1px solid var(--line-2); }
  .toolswitch button[aria-pressed='true'] { background: var(--accent-soft); color: var(--accent); font-weight: 600; }
  .toolswitch svg { width: 15px; height: 15px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  .toolswitch button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .timeline.blade { cursor: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='22' height='22' viewBox='0 0 24 24' fill='none' stroke='%231c1d1a' stroke-width='1.8' stroke-linecap='round'%3E%3Cpath d='M6 3a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM8.5 7.5L20 18M8.5 16.5L20 6'/%3E%3C/svg%3E") 11 11, crosshair; }
  .split { position: absolute; top: 0; bottom: 0; width: 0; border-left: 2px dashed var(--ink); transform: translateX(-1px); }
  .markbar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 13px; } .markinfo { font-variant-numeric: tabular-nums; }
  .markbar kbd { margin-left: 4px; }
  .silences { display: grid; gap: 4px; }
  .pause-mark, .retake-mark { font: 600 12px Archivo, sans-serif; border: 1px solid; border-radius: 10px; padding: 0 7px; cursor: pointer; user-select: none; vertical-align: 2px; }
  .pause-mark { color: #855700; border-color: #85570066; background: #fff6df; }
  .retake-mark { color: var(--accent); border-color: #1f4e4666; background: var(--accent-soft); }
  .pause-mark.gone, .retake-mark.gone { opacity: .45; text-decoration: line-through; }
  .retake { box-shadow: inset 0 -2px 0 #1f4e4655; }
  .cutaway { text-decoration: line-through dotted; color: var(--muted); }
  .playhead { position: absolute; top: -2px; bottom: -2px; width: 2px; background: var(--ink); transform: translateX(-1px); }
  .stats { display: flex; gap: 24px; margin: 0; } .stats div { display: grid; } dt { font-size: 11px; color: var(--muted); } dd { margin: 0; font-weight: 600; font-variant-numeric: tabular-nums; }
  .tools { display: grid; gap: 10px; border-top: 1px solid var(--line); padding-top: 12px; font-size: 13px; }
  .tools .row { display: flex; gap: 8px; flex-wrap: wrap; } .tools label:not(.check) { display: grid; gap: 4px; } .tools p { margin: 0; color: var(--muted); }
  .hint { font-size: 12px; } kbd { font: 11px Archivo, sans-serif; border: 1px solid var(--line-2); border-radius: 4px; padding: 0 4px; }
  .check { display: flex; align-items: center; gap: 6px; font-size: 13px; }
  select, .btn { font: inherit; color: var(--ink); background: var(--bg); border: 1px solid var(--line-2); border-radius: 6px; padding: 7px 10px; }
  .btn { cursor: pointer; } .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; } .btn:disabled { opacity: .5; cursor: default; }
  .link { font: inherit; background: none; border: 0; color: var(--accent); cursor: pointer; padding: 0; text-decoration: underline; }
  .textcol { display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .find { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .find input { font: inherit; font-size: 13px; color: var(--ink); background: var(--bg); border: 1px solid var(--line-2); border-radius: 6px; padding: 7px 10px; }
  .find input[type=search] { flex: 1; min-width: 160px; } .find .time { width: 110px; } .find .invalid { border-color: #923115; }
  .btn.small { padding: 3px 8px; font-size: 12px; }
  .cutbar, .nudge { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; font-size: 13px; } .nudge span { display: inline-flex; align-items: center; gap: 4px; }
  .retakes summary { cursor: pointer; font-weight: 600; } .retakes ul { margin: 6px 0 0; padding-left: 18px; display: grid; gap: 4px; } .retakes li.done { opacity: .6; }
  .latency { display: grid; gap: 4px; } .latency input { width: 100%; accent-color: var(--accent); }
  .hit { box-shadow: inset 0 -2px 0 var(--accent); } .hit.struck { background: #f3c9b8; }
  .doc { font: 17px/1.75 Archivo, sans-serif; max-height: calc(100dvh - 250px); overflow: auto; padding-right: 12px; user-select: text; }
  .para { margin: 0 0 14px; content-visibility: auto; contain-intrinsic-size: auto 90px; } .para.hidden-deleted { display: none; }
  .speaker { display: block; font-size: 12px; font-weight: 600; color: var(--muted); user-select: none; }
  .doc span[data-id] { cursor: text; border-radius: 3px; }
  .doc span[data-id]:hover { background: var(--accent-soft); }
  .struck { text-decoration: line-through; color: var(--muted); background: #9231151a; }
  .sound { color: #6a4a00; font-style: italic; font-size: 14px; }
  .current { background: var(--mark); color: var(--ink); } .current.struck { background: #f3c9b8; }
  @media (max-width: 900px) { .layout { grid-template-columns: 1fr; } .player { position: static; } .doc { max-height: none; } }
</style>
