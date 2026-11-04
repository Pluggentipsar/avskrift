<script lang="ts">
  import { createEta, fmtLeft } from '$lib/eta';
  import { activeWork } from '$lib/work';
  // A progress bar with percent and an estimate of the time left.
  let { pct, label = '' }: { pct: number | null; label?: string } = $props();
  const eta = createEta();
  let left = $state<number | null>(null);
  const paused = $derived(!!$activeWork?.paused);
  $effect(() => { left = eta.update(pct, Date.now(), paused); });
  // Re-estimate between progress reports too, so the time counts down during slow stretches.
  $effect(() => {
    const timer = setInterval(() => { if (pct != null) left = eta.update(pct, Date.now(), paused); }, 5000);
    return () => clearInterval(timer);
  });
</script>

{#if pct != null}
  <div class="eta" role="status" aria-live="polite">
    {#if label}<span class="label">{label}</span>{/if}
    <div class="bar" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(pct)} aria-label={label || 'Förlopp'}><div style:width="{Math.max(0, Math.min(100, pct))}%"></div></div>
    <span class="nums">{Math.round(pct)} % · {paused ? 'pausat' : fmtLeft(left)}</span>
  </div>
{/if}

<style>
  .eta { display: grid; gap: 5px; font-size: 13px; color: var(--muted); min-width: 0; }
  .label { color: var(--ink); }
  .bar { height: 7px; border-radius: 4px; background: var(--line); overflow: hidden; }
  .bar div { height: 100%; background: var(--accent); transition: width .25s; }
  .nums { font-variant-numeric: tabular-nums; }
</style>
