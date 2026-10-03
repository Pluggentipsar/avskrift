<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { openUrl } from '@tauri-apps/plugin-opener';
  let { version, blocked, beforeInstall, onclose }: {
    version: string;
    /** Why installing must wait (recording, background work), or empty when it may proceed. */
    blocked: string;
    beforeInstall: () => Promise<boolean>;
    onclose: () => void;
  } = $props();
  type Info = { current: string; installed: boolean; variant: string; available: { version: string; notes: string | null } | null };
  const CHANNEL_KEY = 'avskrift.updateChannel';
  let dialog: HTMLDialogElement;
  // A pre-release follows pre-releases by default; the viewer's own choice is remembered.
  // svelte-ignore state_referenced_locally
  let channel = $state<'stable' | 'beta'>(version.includes('-') ? 'beta' : 'stable');
  let info = $state<Info | null>(null), checking = $state(false), installing = $state(false), error = $state('');
  let received = $state(0), total = $state<number | null>(null);
  try { const saved = localStorage.getItem(CHANNEL_KEY); if (saved === 'stable' || saved === 'beta') channel = saved; } catch { /* storage unavailable */ }
  onMount(() => {
    dialog.showModal();
    const stop = listen<{ received: number; total: number | null }>('avskrift:update-progress', e => { received = e.payload.received; total = e.payload.total; });
    return () => { stop.then(f => f()); };
  });
  function setChannel(value: 'stable' | 'beta') {
    channel = value; info = null; error = '';
    try { localStorage.setItem(CHANNEL_KEY, value); } catch { /* storage unavailable */ }
  }
  async function check() {
    checking = true; error = ''; info = null;
    try { info = await invoke<Info>('check_update', { channel }); } catch (e) { error = String(e); } finally { checking = false; }
  }
  async function install() {
    if (blocked || installing) return;
    if (!(await beforeInstall())) return;
    installing = true; error = ''; received = 0; total = null;
    // On success the installer closes and restarts the app; this only returns on failure.
    try { await invoke('install_update', { channel }); } catch (e) { error = String(e); installing = false; }
  }
  const mb = (bytes: number) => (bytes / 1024 ** 2).toLocaleString('sv-SE', { maximumFractionDigits: 0 });
</script>

<dialog bind:this={dialog} aria-labelledby="update-title" onclose={onclose} oncancel={(e) => { if (installing) e.preventDefault(); }}>
  <header>
    <h2 id="update-title">Uppdateringar</h2>
    <button onclick={() => dialog.close()} aria-label="Stäng" disabled={installing}>×</button>
  </header>
  <p>Du har version {version}{info ? ` (${info.variant === 'vulkan' ? 'GPU, Vulkan' : 'CPU'})` : ''}. Appen söker bara efter uppdateringar när du ber om det.</p>
  <fieldset disabled={checking || installing}>
    <legend>Vilka versioner</legend>
    <label><input type="radio" name="channel" checked={channel === 'stable'} onchange={() => setChannel('stable')} /> Stabila versioner</label>
    <label><input type="radio" name="channel" checked={channel === 'beta'} onchange={() => setChannel('beta')} /> Även förhandsversioner</label>
  </fieldset>
  <button class="primary" onclick={check} disabled={checking || installing}>{checking ? 'Söker…' : 'Sök efter uppdatering'}</button>

  {#if info && !info.available}
    <p class="ok" role="status">Du har den senaste versionen.</p>
  {:else if info?.available}
    <section class="found" aria-label="Ny version">
      <h3>Version {info.available.version} finns</h3>
      {#if info.available.notes}<div class="notes">{info.available.notes}</div>{/if}
      {#if info.installed}
        {#if installing}
          <p role="status">{total ? `Hämtar ${mb(received)} av ${mb(total)} MB…` : 'Hämtar uppdateringen…'}</p>
          <progress value={total ? received : undefined} max={total ?? undefined} aria-label="Hämtning av uppdateringen"></progress>
        {:else}
          <button class="primary" onclick={install} disabled={!!blocked}>Ladda ner och installera</button>
          <p class="hint">{blocked || 'Appen stängs, uppdateras och startar om. Ditt arbete är sparat på datorn och påverkas inte.'}</p>
        {/if}
      {:else}
        <p class="hint">Den här kopian är portabel och kan inte uppdatera sig själv. Hämta den nya ZIP-filen, eller installationsprogrammet så går nästa uppdatering med ett klick.</p>
        <button onclick={() => openUrl(`https://github.com/Pluggentipsar/avskrift/releases/tag/v${info!.available!.version}`)}>Öppna releasen på GitHub</button>
      {/if}
    </section>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</dialog>

<style>
  dialog { box-sizing: border-box; width: min(560px, calc(100vw - 32px)); max-height: calc(100dvh - 32px); padding: 28px; border: 1px solid var(--line); border-radius: 12px; background: var(--bg); color: var(--ink); font: 14px/1.6 Archivo, sans-serif; }
  dialog::backdrop { background: #1c1d1a66; }
  header { display: flex; justify-content: space-between; align-items: start; gap: 20px; }
  h2 { font: 34px 'Instrument Serif', serif; margin: 0 0 8px; }
  h3 { font-size: 16px; margin: 0 0 8px; }
  p { color: var(--muted); margin: 8px 0; }
  button { font: inherit; color: var(--ink); background: var(--bg); border: 1px solid var(--line-2); border-radius: 6px; padding: 9px 14px; cursor: pointer; }
  header > button { font-size: 22px; padding: 0 12px; }
  button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  button:disabled { opacity: .55; cursor: default; }
  fieldset { border: 0; padding: 0; margin: 16px 0; display: grid; gap: 6px; }
  legend { font-weight: 500; margin-bottom: 6px; }
  label { display: flex; gap: 8px; align-items: center; }
  input { accent-color: var(--accent); }
  .found { border-top: 1px solid var(--line); margin-top: 18px; padding-top: 16px; }
  .notes { white-space: pre-wrap; max-height: 220px; overflow: auto; background: var(--canvas); border: 1px solid var(--line); border-radius: 6px; padding: 10px 12px; margin-bottom: 12px; font-size: 13px; }
  .ok { color: var(--accent); }
  .hint { font-size: 13px; }
  .error { color: #923115; }
  progress { width: 100%; accent-color: var(--accent); }
  :is(button, input):focus-visible { outline: 2px solid var(--accent); outline-offset: 3px; }
</style>
