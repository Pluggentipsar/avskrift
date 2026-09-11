<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import type { DictationSnapshot } from "$lib/dictation";
  let snapshot = $state<DictationSnapshot | null>(null);
  let clock = $state(Date.now());
  const elapsed = $derived(snapshot?.startedAt ? Math.max(0, Math.floor((clock - snapshot.startedAt) / 1000)) : 0);
  function update(next: DictationSnapshot) { if (!snapshot || next.revision >= snapshot.revision) snapshot = next; }
  onMount(() => {
    let disposed = false;
    const listener = listen<DictationSnapshot>("avskrift:dictation", e => update(e.payload));
    void listener.then(() => invoke<DictationSnapshot>("dictation_snapshot")).then(s => { if (!disposed) update(s); });
    const timer = setInterval(() => clock = Date.now(), 500);
    return () => { disposed = true; clearInterval(timer); void listener.then(stop => stop()); };
  });
</script>

<div class="indicator" role="status" aria-live="polite">
  <div class="top"><span class:recording={snapshot?.phase === "recording"}></span><strong>AVskrift</strong>
    {#if snapshot?.phase === "recording"}<time>{Math.floor(elapsed / 60)}:{String(elapsed % 60).padStart(2, "0")}</time>{/if}
  </div>
  <p>{snapshot?.message ?? "Förbereder diktering…"}</p>
  {#if snapshot?.phase === "recording"}<small>{snapshot.inputMode === "hold" ? "Släpp Ctrl+Shift+Space för att transkribera" : snapshot.inputMode === "toggle" ? "Ctrl+Alt+Space för att stoppa" : "Stoppa i AVskrift"}</small>{/if}
</div>

<style>
  :global(body) { margin: 0; background: #1a1a1d; color: #fff; font: 14px "Segoe UI", sans-serif; overflow: hidden; }
  .indicator { box-sizing: border-box; height: 112px; padding: 14px 18px; border: 1px solid #45454d; }
  .top { display: flex; align-items: center; gap: 9px; } .top span { width: 8px; height: 8px; background: #b5b2ff; border-radius: 50%; } .top span.recording { background: #ff7888; }
  strong { font-size: 12px; } time { margin-left: auto; font-variant-numeric: tabular-nums; }
  p { margin: 9px 0 5px; line-height: 1.3; font-size: 13px; } small { color: #c1c1cb; font-size: 11px; }
</style>
