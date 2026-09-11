<script lang="ts">
  import { onMount } from 'svelte';
  export type ExportChoice = { id: string; label: string; formats: string[]; status: string; timestamps?: boolean; appendTranscript?: boolean; sections?:{id:string;label:string}[] };
  let { choices, initial, prepare, onsave, onclose }: {
    choices: ExportChoice[]; initial: string;
    prepare: (id: string, format: string, timestamps: boolean, append: boolean, sections?:string[]) => Promise<string>;
    onsave: (text: string, id: string, format: string) => Promise<boolean>; onclose: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  let source = $state('');
  let excluded=$state<string[]>([]);
  let format = $state('txt');
  let timestamps = $state(false);
  let append = $state(false);
  let text = $state('');
  let pending = $state(false);
  let saving = $state(false);
  let error = $state('');
  let notice = $state('');
  let request = 0;
  const selected = $derived(choices.find(c => c.id === source) ?? choices[0]);
  const formatNames: Record<string,string> = {txt:'Text (.txt)',docx:'Word (.docx)',srt:'Undertext (.srt)',vtt:'Undertext (.vtt)','vtt-words':'Undertext per ord (.vtt)'};
  async function refresh() {
    const token = ++request;
    if (!selected.formats.includes(format)) format = selected.formats[0];
    pending = true; error = ''; notice = ''; text = '';
    try {
      const result = await prepare(source, format, !!selected.timestamps && timestamps, !!selected.appendTranscript && append, selected.sections?.filter(s=>!excluded.includes(s.id)).map(s=>s.id));
      if (token === request) text = result;
    } catch(e) { if (token === request) error = String(e); }
    finally { if (token === request) pending = false; }
  }
  onMount(() => { source = initial; dialog.showModal(); void refresh(); return () => { request++; }; });
  async function copy() {
    try { await navigator.clipboard.writeText(text); notice = 'Kopierat till urklipp.'; } catch(e) { error = String(e); }
  }
  async function save() {
    saving = true; error = ''; notice = '';
    try { if (await onsave(text, source, format)) notice = 'Filen är sparad på datorn.'; }
    catch(e) { error = String(e); }
    finally { saving = false; }
  }
</script>

<dialog bind:this={dialog} aria-labelledby="export-title" onclose={onclose} oncancel={e => { if(saving) e.preventDefault(); }}>
  <div class="head"><div><h2 id="export-title">Granska och exportera</h2><p>Välj vilken text du vill ta med dig.</p></div><button class="close" disabled={saving} onclick={() => dialog.close()} aria-label="Stäng export">×</button></div>
  <fieldset disabled={saving}>
    <div class="options"><label>Innehåll<select aria-label="Innehåll" bind:value={source} onchange={refresh}>{#each choices as c}<option value={c.id}>{c.label}</option>{/each}</select></label><label>Format<select aria-label="Format" bind:value={format} onchange={refresh}>{#each selected.formats as f}<option value={f}>{formatNames[f]}</option>{/each}</select></label></div>
    <div class="checks">{#if selected.timestamps && ['txt','docx'].includes(format)}<label><input type="checkbox" bind:checked={timestamps} onchange={refresh} /> Ta med tidsstämplar</label>{/if}{#if selected.appendTranscript}<label><input type="checkbox" bind:checked={append} onchange={refresh} /> Bifoga originaltranskriptet</label>{/if}</div>
    {#if selected.sections}<div class="checks">{#each selected.sections as part}<label><input type="checkbox" checked={!excluded.includes(part.id)} onchange={e=>{excluded=e.currentTarget.checked?excluded.filter(id=>id!==part.id):[...excluded,part.id];void refresh();}}/>{part.label}</label>{/each}</div>{/if}
  </fieldset>
  <p class="status">{selected.status}{#if append && selected.appendTranscript} Originaltranskriptet ingår också.{/if}</p>
  {#if error}<p class="error" role="alert">{error}</p><button onclick={refresh} disabled={saving}>Försök igen</button>{/if}
  <label class="preview-label" for="export-preview">{format === 'docx' ? 'Textinnehåll i Word-filen' : 'Förhandsvisning'}</label>
  <textarea id="export-preview" readonly value={pending ? 'Förbereder förhandsvisning…' : text} aria-busy={pending}></textarea>
  <div class="footer"><span role="status">{notice}</span><button onclick={copy} disabled={pending || saving || !text || !!error}>Kopiera text</button><button class="primary" onclick={save} disabled={pending || saving || !text || !!error}>{saving ? 'Sparar…' : 'Spara fil…'}</button></div>
</dialog>

<style>
  dialog { box-sizing:border-box; width:min(850px,calc(100vw - 32px)); max-height:calc(100dvh - 32px); padding:28px; border:1px solid var(--line); border-radius:12px; color:var(--ink); background:var(--bg); font:14px/1.5 Archivo,sans-serif; box-shadow:0 18px 80px #17172b33; }
  dialog::backdrop { background:#17172b66; } .head { display:flex; justify-content:space-between; gap:20px; } h2 { font:32px/1.2 'Instrument Serif',serif; margin:0; } p { color:var(--muted); } .close { font-size:24px; align-self:flex-start; border:0; }
  fieldset { border:0; margin:12px 0; padding:0; } .options { display:grid; grid-template-columns:1fr 1fr; gap:18px; } .options label { display:grid; gap:7px; } select { width:100%; }
  input,select,textarea,button { font:inherit; color:inherit; } button,select { border:1px solid var(--line-2); background:var(--bg); border-radius:7px; padding:10px 13px; } button { cursor:pointer; } button:disabled { opacity:.5; cursor:default; }
  .checks { display:flex; flex-wrap:wrap; gap:18px; margin-top:15px; } .checks label { display:flex; align-items:center; gap:8px; } input { accent-color:var(--accent); }
  .status { background:var(--nav-bg); padding:12px; border-radius:7px; } .preview-label { display:block; margin-bottom:7px; } textarea { box-sizing:border-box; width:100%; min-height:230px; height:32vh; resize:vertical; border:1px solid var(--line-2); background:var(--bg); border-radius:7px; padding:16px; line-height:1.7; }
  .footer { display:flex; flex-wrap:wrap; gap:10px; align-items:center; margin-top:20px; } .footer span { flex:1; color:#296144; } .primary { background:var(--accent); color:white; border-color:var(--accent); } .error { color:#923115; }
  @media(max-width:550px) { dialog { padding:18px; } .options { grid-template-columns:1fr; } .footer span { flex-basis:100%; } }
</style>
