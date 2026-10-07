<script lang="ts">
  import { renderMarkdown, hasStructure, htmlToMarkdown, readClipboardMarkdown } from '$lib/markdown';
  // The summary draft as markdown: edit it, see it formatted (tables as tables), and paste text from
  // an AI chat without losing its structure.
  let { value = $bindable(), mode = $bindable('edit'), disabled = false, oninput }: {
    value: string; mode?: 'edit' | 'view'; disabled?: boolean; oninput: () => void;
  } = $props();
  let area = $state<HTMLTextAreaElement | null>(null);
  let pasteChoice = $state<string | null>(null), note = $state('');
  const preview = $derived(mode === 'view' ? renderMarkdown(value) : '');

  function insert(text: string) {
    const el = area;
    if (!el) { value = (value ? value + '\n\n' : '') + text; oninput(); return; }
    const start = el.selectionStart, end = el.selectionEnd;
    value = value.slice(0, start) + text + value.slice(end);
    oninput();
    queueMicrotask(() => { el.focus(); el.selectionStart = el.selectionEnd = start + text.length; });
  }
  /** Formatted text (HTML) becomes markdown; plain text and markdown paste as they are. */
  function onpaste(e: ClipboardEvent) {
    const html = e.clipboardData?.getData('text/html');
    if (!html || !hasStructure(html)) return;
    e.preventDefault();
    insert(htmlToMarkdown(html).trim());
    note = 'Formateringen behölls som markdown.';
  }
  async function pasteFromAi() {
    note = '';
    let text = '';
    try { text = (await readClipboardMarkdown()).trim(); } catch { /* no clipboard access */ }
    if (!text) { mode = 'edit'; queueMicrotask(() => area?.focus()); note = 'Urklippet kunde inte läsas här. Klicka i rutan och tryck Ctrl+V.'; return; }
    if (value.trim()) { pasteChoice = text; return; }
    value = text; oninput(); mode = 'view'; note = 'Inklistrat från urklipp.';
  }
  function applyPaste(how: 'replace' | 'append') {
    if (pasteChoice == null) return;
    value = how === 'replace' ? pasteChoice : value.trimEnd() + '\n\n' + pasteChoice;
    pasteChoice = null; oninput(); mode = 'view'; note = 'Inklistrat från urklipp.';
  }
</script>

<div class="summary-tools">
  <div class="seg" role="group" aria-label="Visning av sammanfattningen">
    <button aria-pressed={mode === 'edit'} onclick={() => (mode = 'edit')}>Redigera</button>
    <button aria-pressed={mode === 'view'} onclick={() => (mode = 'view')}>Visa formaterat</button>
  </div>
  <button class="tool" onclick={pasteFromAi} {disabled} title="Klistra in en sammanfattning du gjort med en annan AI. Tabeller, rubriker och listor behålls.">Klistra in från AI</button>
</div>
{#if pasteChoice != null}
  <div class="choice" role="group" aria-label="Var ska den inklistrade texten hamna?">
    <span>Det finns redan ett utkast. Vad vill du göra med texten från urklipp?</span>
    <button class="tool" onclick={() => applyPaste('replace')}>Ersätt utkastet</button>
    <button class="tool" onclick={() => applyPaste('append')}>Lägg till sist</button>
    <button class="tool" onclick={() => (pasteChoice = null)}>Avbryt</button>
  </div>
{/if}
{#if note}<p class="hint" role="status">{note}</p>{/if}
{#if mode === 'edit'}
  <textarea bind:this={area} {disabled} class="summary-edit" bind:value oninput={() => { note = ''; oninput(); }} {onpaste} aria-label="Sammanfattning – redigerbart utkast" spellcheck="true"></textarea>
{:else}
  <!-- Sanitised in renderMarkdown; pasted text never runs as markup. -->
  <div class="summary-view" aria-label="Sammanfattning – formaterad">{@html preview}</div>
{/if}

<style>
  .summary-tools { display: flex; align-items: center; justify-content: space-between; gap: 10px; flex-wrap: wrap; margin-bottom: 10px; }
  .seg { display: inline-flex; border: 1px solid var(--line-2); border-radius: 7px; overflow: hidden; }
  .seg button { font: inherit; font-size: 13px; border: 0; background: var(--bg); color: var(--muted); padding: 6px 12px; cursor: pointer; }
  .seg button + button { border-left: 1px solid var(--line-2); }
  .seg button[aria-pressed='true'] { background: var(--accent-soft); color: var(--accent); }
  .tool { font: inherit; font-size: 13px; color: var(--ink); background: var(--bg); border: 1px solid var(--line-2); border-radius: 7px; padding: 6px 12px; cursor: pointer; }
  .tool:hover:not(:disabled) { border-color: var(--accent); } .tool:disabled { opacity: .5; cursor: default; }
  .choice { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-bottom: 10px; padding: 10px 13px; border-radius: 7px; background: var(--accent-soft); font-size: 13.5px; } .choice span { flex: 1; min-width: 220px; }
  .hint { font-size: 13px; color: var(--muted); margin: 0 0 8px; }
  .summary-edit { box-sizing: border-box; width: 100%; min-height: 260px; resize: vertical; padding: 16px 20px; font: 16px/1.7 Archivo, sans-serif; color: var(--ink); background: var(--bg); border: 1px solid var(--line-2); border-radius: 7px; }
  .tool:focus-visible, .summary-edit:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .summary-view { box-sizing: border-box; min-height: 260px; padding: 16px 20px; background: var(--bg); border: 1px solid var(--line); border-radius: 7px; line-height: 1.65; font-size: 15px; overflow-wrap: anywhere; }
  .summary-view :global(h1) { font: 28px/1.2 'Instrument Serif', serif; margin: 6px 0 10px; }
  .summary-view :global(h2) { font-size: 18px; margin: 18px 0 6px; }
  .summary-view :global(h3) { font-size: 15px; margin: 14px 0 4px; }
  .summary-view :global(table) { border-collapse: collapse; margin: 10px 0; width: 100%; font-size: 14px; display: block; overflow-x: auto; }
  .summary-view :global(th), .summary-view :global(td) { border: 1px solid var(--line-2); padding: 6px 10px; text-align: left; vertical-align: top; }
  .summary-view :global(th) { background: var(--canvas); }
  .summary-view :global(blockquote) { margin: 8px 0; padding-left: 12px; border-left: 3px solid var(--line-2); color: var(--muted); }
  .summary-view :global(code) { font-family: Consolas, monospace; font-size: 13px; background: var(--canvas); padding: 1px 4px; border-radius: 3px; }
  .seg button:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
</style>
