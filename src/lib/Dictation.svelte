<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { tick } from "svelte";
  import RewriteDialog from './RewriteDialog.svelte';
  import { createSaveQueue } from './save-queue';
  import type { DictationSnapshot, DictationSettings, DictationEntry } from "./dictation";
  let { snapshot, models, onmodels, ontext, textModel, textReady, dirty = $bindable(false) }: {
    textModel: string;
    textReady: boolean;
    snapshot: DictationSnapshot | null;
    models: { id: string; label: string; downloaded: boolean }[];
    onmodels: () => void;
    ontext: (text: string) => void;
    dirty?: boolean;
  } = $props();
  let error = $state("");
  let notice = $state("");
  let search = $state("");
  let pending = $state(false);
  let drafts = $state<Record<string, string>>({});
  let rewriting = $state<{id:string;text:string;original:string}|null>(null);
  async function openRewrite(entry:DictationEntry) {
    if(!textReady){onmodels();return;}
    if(!(await flushChanges()))return;
    const current=snapshot?.entries.find(e=>e.id===entry.id);
    if(current)rewriting={id:current.id,text:current.text,original:current.originalText??current.text};
  }
  async function acceptRewrite(text:string) {
    if(!rewriting)return false;
    const current=snapshot?.entries.find(e=>e.id===rewriting!.id);
    if(!current)throw Error('Diktatet finns inte kvar.');
    await saves.enqueue(()=>invoke('edit_dictation',{id:current.id,text,saved:current.saved,expectedText:rewriting!.text}));
    notice=current.saved?'Den godkända texten är sparad på datorn.':'Den godkända texten finns i sessionen.';
    return true;
  }
  let selectedId = $state<string | null>(null);
  let entriesElement = $state<HTMLElement>();
  export async function openEntry(id: string) {
    search = ""; selectedId = id;
    await tick();
    const article = Array.from(entriesElement?.querySelectorAll<HTMLElement>('article') ?? []).find(el => el.dataset.entryId === id);
    article?.scrollIntoView({block:'nearest'});
    article?.querySelector('textarea')?.focus({preventScroll:true});
  }
  const saves=createSaveQueue();
  let saveTimer: ReturnType<typeof setTimeout> | null=null;
  $effect(()=>{dirty=Object.keys(drafts).length>0;});
  function change(entry:DictationEntry,text:string) {
    drafts[entry.id]=text;
    notice=entry.saved?'Ändringen väntar på att sparas på datorn…':'Ändringen behålls under sessionen…';
    if(saveTimer)clearTimeout(saveTimer);
    saveTimer=setTimeout(()=>{saveTimer=null;void flushChanges();},500);
  }
  export async function flushChanges():Promise<boolean> {
    if(saveTimer){clearTimeout(saveTimer);saveTimer=null;}
    return saves.enqueue(async()=>{
      for(const [id,text] of Object.entries(drafts)) {
        const entry=snapshot?.entries.find(e=>e.id===id);
        if(!entry){delete drafts[id];continue;}
        try {
          await invoke('edit_dictation',{id,text,saved:entry.saved});
          if(drafts[id]===text)delete drafts[id];
          notice=entry.saved?'Ändringen är sparad på datorn.':'Ändringen finns kvar under sessionen.';
          error='';
        } catch(e){error=String(e);return false;}
      }
      return true;
    });
  }
  const active = $derived(!!snapshot && snapshot.phase !== "idle");
  const entries = $derived(snapshot?.entries.filter(e => (drafts[e.id] ?? e.text).toLocaleLowerCase("sv").includes(search.toLocaleLowerCase("sv"))) ?? []);
  const downloaded = $derived(models.some(m => m.id === snapshot?.settings.model && m.downloaded));

  async function perform(action: () => Promise<unknown>) {
    error = ""; notice = ""; pending = true;
    try { await action(); } catch (e) { error = String(e); }
    finally { pending = false; }
  }
  function configure(change: Partial<DictationSettings>) {
    if (!snapshot) return;
    void perform(() => invoke("configure_dictation", { settings: { ...snapshot!.settings, ...change } }));
  }
  function update(entry: DictationEntry, saved = entry.saved) {
    const submitted = drafts[entry.id] ?? entry.text;
    void perform(async () => {
      await saves.enqueue(()=>invoke("edit_dictation", { id: entry.id, text: submitted, saved }));
      if (drafts[entry.id] === submitted) delete drafts[entry.id];
      notice = saved ? "Diktatet är sparat på datorn." : "Ändringen behålls under sessionen.";
    });
  }
  async function copy(entry: DictationEntry) {
    await perform(async () => {
      await navigator.clipboard.writeText(drafts[entry.id] ?? entry.text);
      notice = "Kopierat. Klistra in där du vill ha texten.";
    });
  }
</script>

<div class="dictation-page">
  <div class="heading"><h2>Diktering</h2><p>Tala där du skriver. Hitta orden här igen.</p></div>
  {#if !snapshot}
    <p role="status">Hämtar dikteringsinställningar…</p>
  {:else}
    <div class="workspace">
      <aside aria-label="Dikteringsinställningar">
        <div class="recorder">
          <div class="state" role="status"><span class:recording={snapshot.phase === "recording"} class:working={active}></span>{snapshot.message}</div>
          <div class="controls">
            <button class="primary" disabled={pending || !snapshot.supported || (!active && !downloaded) || snapshot.phase === "processing"}
              onclick={() => perform(() => invoke("toggle_dictation"))}>
              {snapshot.phase === "recording" || snapshot.phase === "starting" ? "Stoppa och transkribera" : snapshot.phase === "processing" ? "Bearbetar…" : "Starta diktat här"}
            </button>
            {#if active}<button disabled={pending} onclick={() => perform(() => invoke("cancel_dictation"))}>Avbryt</button>{/if}
          </div>
          <p class="hint">Knappen samlar text här. Håll Ctrl+Shift+Space för att diktera i ett annat program och släpp för att transkribera. Ett diktat kan vara upp till fem minuter.</p>
        </div>
        {#if !snapshot.supported}<p class="warning">Diktering i andra program finns i första versionen för Windows.</p>{/if}
        <p class="hint">Textbearbetning: {textModel}{textReady?'':' – behöver hämtas'}</p><button onclick={onmodels}>Modeller för tal och text</button>
        <details class="settings" open={!downloaded}>
        <summary>Ljud, genvägar och sparande</summary>
        <fieldset disabled={active || pending || !snapshot.supported}>
          <legend>Kortkommandon</legend>
          <label class="check"><input type="checkbox" checked={snapshot.settings.enabled} onchange={e => configure({ enabled: e.currentTarget.checked })} /> Aktivera i alla program</label>
          <p><strong>Ctrl+Shift+Space</strong><br />Håll inne för att tala. Släpp för att transkribera.</p>
          <p><strong>Ctrl+Alt+Space</strong><br />Tryck för att starta. Tryck igen för att stoppa och transkribera.</p>
          {#if snapshot.settings.enabled}
            <p class="hint">Håll inne: {snapshot.holdShortcutReady ? "aktivt" : "inte aktivt"}. Start/stopp: {snapshot.toggleShortcutReady ? "aktivt" : "inte aktivt"}.</p>
          {/if}
          {#if snapshot.shortcutError}<p class="warning" role="alert">{snapshot.shortcutError}</p>
          {:else if snapshot.settings.enabled}<p class="hint">Minimera AVskrift och placera markören i ditt program.</p>{/if}
          <label class="check"><input type="checkbox" checked={snapshot.settings.autoInsert} onchange={e => configure({ autoInsert: e.currentTarget.checked })} /> Infoga i textfältet efter stopp</label>
          <p class="hint">Om textfältet inte kan kontrolleras eller fokus ändras finns texten kvar här. Urklippet lämnas orört vid automatisk infogning.</p>
        </fieldset>
        <fieldset disabled={active || pending}>
          <legend>Talmodell</legend>
          <p class="hint">{snapshot.backend}</p>
          {#if snapshot.preparingModel}<p class="hint" role="status">Förbereder talmodellen… Första GPU-starten kan ta längre tid. Du kan börja tala medan modellen förbereds.</p>{/if}
          <p>{models.find(m=>m.id===snapshot.settings.model)?.label??snapshot.settings.model}</p>
          <button onclick={onmodels}>{downloaded?'Byt eller hämta modell':'Hämta talmodell'}</button>
          <p class="hint">Base är snabb. Small ger ofta bättre text men tar längre tid. Ett pågående transkriberingsjobb blir klart först.</p>
        </fieldset>
        <fieldset disabled={active || pending}>
          <legend>Spara diktat</legend>
          <label class="check"><input type="checkbox" checked={snapshot.settings.saveHistory} onchange={e => configure({ saveHistory: e.currentTarget.checked })} /> Spara nya diktat automatiskt</label>
          <p class="hint">Sparade diktat finns kvar efter omstart tills du tar bort dem. Övriga finns bara under den här sessionen. Ljudet sparas inte.</p>
        </fieldset>
        </details>
      </aside>
      <section class="entries" aria-label="Diktat" bind:this={entriesElement}>
        <div class="list-heading"><h3>Dina diktat <span>{snapshot.entries.length}</span></h3><input aria-label="Sök i diktat" type="search" placeholder="Sök i dina diktat…" bind:value={search} /></div>
        {#if error}<p class="warning" role="alert">{error}</p>{/if}
        {#if snapshot.storageError}<p class="warning" role="alert">{snapshot.storageError}</p>{/if}
        {#if notice}<p class="notice" role="status">{notice}</p>{/if}
        {#if !entries.length}
          <div class="empty"><h3>{search ? "Inga diktat matchar sökningen" : "Här landar dina ord"}</h3><p>{search ? "Prova ett annat sökord." : "Starta ett diktat här eller använd kortkommandot i en textruta. Texten finns kvar här även när infogningen inte når fram."}</p></div>
        {/if}
        {#each entries as entry (entry.id)}
          <article data-entry-id={entry.id}>
            <div class="entry-meta"><time datetime={new Date(entry.createdAt).toISOString()}>{new Date(entry.createdAt).toLocaleString("sv-SE", { dateStyle: "medium", timeStyle: "short" })}</time><span>{entry.saved ? "Sparat på datorn" : "Den här sessionen"}</span></div>
            <details class="dictat-text" open={entry.id === selectedId || entry.id === snapshot.entries[0]?.id}>
              <summary>{entry.id === snapshot.entries[0]?.id ? "Senaste diktatet" : entry.text.slice(0, 90) + (entry.text.length > 90 ? "…" : "")}</summary>
              <textarea aria-label="Diktat från {new Date(entry.createdAt).toLocaleString('sv-SE')}" rows="4" value={drafts[entry.id] ?? entry.text} oninput={e => change(entry,e.currentTarget.value)}></textarea>
            </details>
            <p class="delivery">{entry.delivery}</p>
            <div class="entry-actions">
              <button class="primary" disabled={pending} onclick={() => copy(entry)}>Kopiera</button>
              <button disabled={pending} onclick={() => openRewrite(entry)}>Bearbeta text</button>
              {#if drafts[entry.id] !== undefined}<button disabled={pending} onclick={() => update(entry)}>Behåll ändring</button>{/if}
              <button disabled={pending} onclick={() => update(entry, !entry.saved)}>{entry.saved ? "Behåll bara i sessionen" : "Spara diktat"}</button>
              <button disabled={pending} onclick={() => ontext(drafts[entry.id] ?? entry.text)}>Avidentifiera</button>
              <button class="remove" disabled={pending} onclick={() => perform(() => invoke("delete_dictation", { id: entry.id }))}>Ta bort</button>
            </div>
          </article>
        {/each}
      </section>
    </div>
  {/if}
</div>

{#if rewriting}<RewriteDialog text={rewriting.text} original={rewriting.original} model={textModel} onaccept={acceptRewrite} onclose={()=>rewriting=null}/>{/if}

<style>
  .dictation-page { width:100%; box-sizing:border-box; max-width: 1180px; margin: 0 auto; padding: 36px 30px 60px; color: var(--ink); font: 14px Archivo, sans-serif; }
  .heading { margin-bottom: 30px; } .heading h2 { font: 44px/1.1 "Instrument Serif", serif; margin: 0 0 10px; } .heading p { color: #696a6f; margin: 0; }
  .workspace { display: grid; grid-template-columns: 260px minmax(0, 1fr); gap: 28px; align-items: start; }
  aside { border-right: 1px solid #e8e8eb; padding-right: 28px; } .recorder { padding-bottom: 22px; }
  .state { display: flex; align-items: center; gap: 9px; min-height: 40px; line-height: 1.5; margin-bottom: 14px; }
  .state span { width: 9px; height: 9px; flex-shrink: 0; border-radius: 50%; background: #a6a7ad; } .state span.working { background: #3a36b0; } .state span.recording { background: #b4233b; }
  .controls, .entry-actions { display: flex; flex-wrap: wrap; gap: 8px; }
  button { font: inherit; color: #1a1a1d; background: white; border: 1px solid #dadadf; border-radius: 6px; padding: 9px 12px; cursor: pointer; } button:hover { background: #f4f4f7; }
  button.primary { background: #3a36b0; color: white; border-color: #3a36b0; } button.primary:hover { background: #2e2b8f; } button:disabled { opacity: .5; cursor: default; }
  :is(button, input, textarea):focus-visible { outline: 2px solid #3a36b0; outline-offset: 3px; }
  fieldset { margin: 0; padding: 20px 0; border: 0; border-top: 1px solid #e8e8eb; min-width: 0; } legend { font-weight: 600; padding: 0 6px 0 0; }
  .check { display: flex; gap: 8px; align-items: start; line-height: 1.5; margin: 12px 0; } input[type=checkbox] { accent-color: #3a36b0; margin-top: 3px; }
  input[type=search], textarea { box-sizing: border-box; font: inherit; padding: 10px; border: 1px solid #dadadf; border-radius: 6px; background: white; color: inherit; }
  .hint { color: var(--muted); font-size: 13px; line-height: 1.65; margin: 12px 0 0; }
  .list-heading { display: flex; gap: 16px; justify-content: space-between; align-items: center; margin-bottom: 20px; } .list-heading h3 { font-size: 18px; margin: 0; } .list-heading h3 span { color: #696a6f; font-weight: 400; margin-left: 5px; } .list-heading input { width: 210px; }
  article { border-top: 1px solid #e8e8eb; padding: 22px 0; } .entry-meta { display: flex; gap: 12px; justify-content: space-between; color: #696a6f; font-size: 12px; margin-bottom: 12px; }
  textarea { width: 100%; resize: vertical; min-height: 96px; line-height: 1.7; } .delivery { font-size: 12px; color: #696a6f; line-height: 1.5; margin: 8px 0 14px; } .entry-actions button { font-size: 12px; } .remove { margin-left: auto; }
  .empty { padding: 55px 15px; max-width: 430px; } .empty h3 { font: 30px "Instrument Serif", serif; margin: 0 0 12px; } .empty p { line-height: 1.7; color: #696a6f; }
  .warning { color: #923115; background: #fff3ea; padding: 12px; line-height: 1.6; border-radius: 6px; } .notice { color: #24543b; line-height: 1.6; }
  .settings summary { cursor:pointer; color:var(--accent); line-height:1.6; padding:12px 0; }
  .dictat-text summary { cursor:pointer; color:var(--ink); line-height:1.6; margin-bottom:14px; overflow-wrap:anywhere; }
  .dictat-text textarea { font-size:16px; }
  .entry-meta,.delivery,.entry-actions button { font-size:13px; }
  .list-heading { flex-wrap:wrap; } .entries { min-width:0; }
  @media (max-width: 800px) { .workspace { grid-template-columns: 1fr; gap: 24px; } aside { border-right: 0; padding-right: 0; } .dictation-page { padding: 24px 20px; } }
  @media (max-width: 450px) { .list-heading { align-items: start; flex-direction: column; } .list-heading input { width: 100%; } .entry-meta { flex-direction: column; gap: 5px; } }
</style>
