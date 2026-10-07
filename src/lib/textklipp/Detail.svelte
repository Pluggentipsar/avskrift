<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { fmt, type Token, type Edge } from './types';
  let { projectId, time, duration, keep, tokens, deleted, fps, active, mark = null, silences = [], onseek, onedge, onrange }: {
    projectId: string; time: number; duration: number; keep: [number, number][]; tokens: Token[];
    deleted: Set<number>; fps: number; active: [number, number] | null;
    /** Range marked for a manual cut, and long silences still in the edit. */
    mark?: [number, number] | null; silences?: [number, number][];
    onseek: (t: number) => void; onedge: (edge: Edge, to: number) => void; onrange: (a: number, b: number) => void;
  } = $props();

  const SPAN = 8; // seconds shown
  let canvas = $state<HTMLCanvasElement>();
  let width = $state(600);
  let view = $state(0); // window start; follows the playhead in steps so the waveform is not refetched every frame
  let bars = $state<Float32Array | number[]>([]);
  let barsFor = '';
  let drag = $state<{ edge: Edge; x: number } | null>(null);
  /** Dragging over the waveform away from a cut edge marks a range; a click (no movement) seeks. */
  let sweep = $state<{ from: number; x: number; moved: boolean } | null>(null);

  $effect(() => {
    const t = time;
    if (drag || sweep) return;
    if (t < view + SPAN * 0.2 || t > view + SPAN * 0.8) view = Math.max(0, Math.min(Math.max(0, duration - SPAN), t - SPAN * 0.35));
  });
  $effect(() => {
    const key = `${projectId}:${view.toFixed(2)}:${width}`;
    if (key === barsFor) return;
    barsFor = key;
    const [a, b, n] = [view, view + SPAN, Math.max(50, Math.round(width))];
    invoke<number[]>('textklipp_waveform', { id: projectId, start: a, end: b, bars: n }).then(v => { if (barsFor === key) bars = v; }).catch(() => {});
  });

  const x = (t: number) => ((t - view) / SPAN) * width;
  const t = (px: number) => view + (px / width) * SPAN;
  const edges = $derived(keep.flatMap(([a, b]): Edge[] => [...(a > 0.001 ? [{ kind: 'in' as const, at: a }] : []), ...(b < duration - 0.001 ? [{ kind: 'out' as const, at: b }] : [])])
    .filter(e => e.at >= view && e.at <= view + SPAN));

  function color(name: string, fallback: string) {
    return (canvas && getComputedStyle(canvas).getPropertyValue(name).trim()) || fallback;
  }
  $effect(() => {
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1, h = 120;
    canvas.width = Math.round(width * dpr); canvas.height = h * dpr;
    const g = canvas.getContext('2d')!;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, width, h);
    const ink = color('--ink', '#1c1d1a'), muted = color('--muted', '#5f605a'), accent = color('--accent', '#1f4e46');
    // Removed stretches.
    let prev = 0;
    for (const [a, b] of [...keep, [duration, duration] as [number, number]]) {
      if (a > prev) { g.fillStyle = '#9231152e'; g.fillRect(x(prev), 0, x(a) - x(prev), h); }
      prev = b;
    }
    // Long silences still in the edit, then the marked range.
    for (const [a, b] of silences) { if (b < view || a > view + SPAN) continue; g.fillStyle = '#85570024'; g.fillRect(x(a), 0, x(b) - x(a), h); }
    const marked = sweep?.moved ? [Math.min(sweep.from, sweep.x), Math.max(sweep.from, sweep.x)] : mark ? [x(mark[0]), x(mark[1])] : null;
    if (marked) { g.fillStyle = '#1f4e4633'; g.fillRect(marked[0], 0, marked[1] - marked[0], h); g.fillStyle = accent; g.fillRect(marked[0] - 1, 0, 2, h); g.fillRect(marked[1] - 1, 0, 2, h); }
    if (active) { g.strokeStyle = '#923115'; g.lineWidth = 1; g.strokeRect(x(active[0]) + 0.5, 0.5, x(active[1]) - x(active[0]) - 1, h - 1); }
    // Waveform, mirrored around the middle of the upper area.
    const mid = 46, amp = 40, bw = width / Math.max(1, bars.length);
    g.fillStyle = muted;
    for (let i = 0; i < bars.length; i++) { const v = bars[i] * amp; g.fillRect(i * bw, mid - v, Math.max(1, bw - 0.5), v * 2 + 1); }
    // Words along the bottom.
    g.font = '12px Archivo, sans-serif'; g.textBaseline = 'alphabetic';
    let lastRight = -Infinity;
    for (const tok of tokens) {
      if (tok.end < view || tok.start > view + SPAN) continue;
      const px = x(tok.start);
      g.fillStyle = deleted.has(tok.id) ? '#923115' : tok.kind === 'sound' ? '#6a4a00' : ink;
      g.fillRect(px, 96, Math.max(1, x(tok.end) - px), 2);
      const label = tok.kind === 'sound' ? `[${tok.text}]` : tok.text;
      if (px > lastRight + 4) { g.fillText(label, px, 114); lastRight = px + g.measureText(label).width; }
    }
    // Cut edges (handles), then the playhead.
    for (const e of edges) {
      const px = drag && drag.edge.at === e.at && drag.edge.kind === e.kind ? drag.x : x(e.at);
      g.fillStyle = '#923115'; g.fillRect(px - 1, 0, 2, h);
      g.beginPath(); const dir = e.kind === 'out' ? 1 : -1; g.moveTo(px, 0); g.lineTo(px + 8 * dir, 0); g.lineTo(px, 10); g.fill();
    }
    g.fillStyle = accent; g.fillRect(x(time) - 1, 0, 2, h);
  });

  function hit(px: number): Edge | null {
    let best: Edge | null = null, d = 7;
    for (const e of edges) { const dd = Math.abs(x(e.at) - px); if (dd < d) { d = dd; best = e; } }
    return best;
  }
  function local(e: PointerEvent) { return e.clientX - (e.currentTarget as HTMLElement).getBoundingClientRect().left; }
  function down(e: PointerEvent) {
    const px = local(e), edge = hit(px);
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    if (edge) drag = { edge, x: px };
    else sweep = { from: px, x: px, moved: false };
  }
  function move(e: PointerEvent) {
    const px = Math.max(0, Math.min(width, local(e)));
    if (drag) drag = { ...drag, x: px };
    else if (sweep) sweep = { ...sweep, x: px, moved: sweep.moved || Math.abs(px - sweep.from) > 4 };
    else (e.currentTarget as HTMLElement).style.cursor = hit(px) ? 'ew-resize' : 'pointer';
  }
  function up() {
    if (sweep) {
      const s = sweep; sweep = null;
      if (s.moved) onrange(t(Math.min(s.from, s.x)), t(Math.max(s.from, s.x)));
      else onseek(t(s.from));
      return;
    }
    if (!drag) return;
    const to = Math.round(t(drag.x) * fps) / fps;
    const edge = drag.edge; drag = null;
    if (Math.abs(to - edge.at) >= 0.5 / fps) onedge(edge, to);
  }
</script>

<div class="detail" bind:clientWidth={width}>
  <canvas bind:this={canvas} data-view={view} data-span={SPAN} aria-label="Detaljvy med vågform, ord och klipp kring uppspelningsmarkören. Dra en röd kant för att flytta ett klipp, eller dra över vågformen för att markera ett avsnitt."
    onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={() => { drag = null; sweep = null; }}></canvas>
  <div class="scale"><span>{fmt(view)}</span>{#if drag}<span class="dragging">{fmt(t(drag.x))}.{String(Math.round((t(drag.x) % 1) * 100)).padStart(2, '0')}</span>{/if}<span>{fmt(view + SPAN)}</span></div>
</div>

<style>
  .detail { display: grid; gap: 2px; user-select: none; min-width: 0; width: 100%; }
  canvas { display: block; width: 100%; height: 120px; border-radius: 6px; background: var(--accent-soft); touch-action: none; cursor: pointer; }
  .scale { display: flex; justify-content: space-between; font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .dragging { color: #923115; font-weight: 600; }
</style>
