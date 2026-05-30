<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import "@fontsource/instrument-serif/400.css";
  import "@fontsource/instrument-serif/400-italic.css";
  import "@fontsource/archivo/400.css";
  import "@fontsource/archivo/500.css";
  import "@fontsource/archivo/600.css";
  import "@fontsource/archivo/700.css";

  // ---- Types mirrored from the Rust side ----
  type WhisperModel = { id: string; label: string; sizeMb: number; downloaded: boolean };
  type Word = { start: number; end: number; text: string };
  type Utterance = { start: number; end: number; speaker: string | null; text: string; words?: Word[] };
  type Transcript = { utterances: Utterance[]; language: string; model: string; diarized: boolean };
  type SpanInfo = { id: number; category: string; source: string; text: string; replacement: string };
  type Segment = { text: string; span: number | null };
  type AnalyzeResult = {
    text: string;
    segments: Segment[];
    spans: SpanInfo[];
    counts: Record<string, number>;
    warnings: string[];
  };

  const CATEGORIES = [
    { key: "person", label: "Person", color: "#e11d48" },
    { key: "personnummer", label: "Personnummer", color: "#be123c" },
    { key: "plats", label: "Plats", color: "#2563eb" },
    { key: "organisation", label: "Organisation", color: "#7c3aed" },
    { key: "telefon", label: "Telefon", color: "#0891b2" },
    { key: "epost", label: "E-post", color: "#059669" },
    { key: "ip_adress", label: "IP-adress", color: "#4f46e5" },
    { key: "tid", label: "Tid", color: "#d97706" },
    { key: "handelse", label: "Händelse", color: "#0d9488" },
    { key: "diagnos", label: "Diagnos", color: "#b45309" },
    { key: "medicin", label: "Medicin", color: "#a21caf" },
    { key: "egen", label: "Egen ordlista", color: "#db2777" },
    { key: "ovrigt", label: "Övrigt (AI)", color: "#64748b" },
  ];
  const ALL_KEYS = CATEGORIES.map((c) => c.key);
  const colorOf = (key: string) => CATEGORIES.find((c) => c.key === key)?.color ?? "#888";

  const IDENTITY = ["person", "personnummer", "telefon", "epost", "ip_adress", "plats", "organisation", "egen", "ovrigt"];
  const PROFILES = [
    { id: "allman", label: "Allmän", cats: IDENTITY },
    { id: "skola", label: "Skola / Elevhälsa", cats: [...IDENTITY, "diagnos", "medicin"] },
    { id: "social", label: "Socialtjänst", cats: [...IDENTITY, "diagnos", "medicin", "handelse"] },
    { id: "allt", label: "Allt", cats: ALL_KEYS },
  ];
  function profileMap(id: string): Record<string, boolean> {
    const set = new Set(PROFILES.find((p) => p.id === id)?.cats ?? ALL_KEYS);
    return Object.fromEntries(ALL_KEYS.map((k) => [k, set.has(k)]));
  }

  const LANGUAGES = [
    { code: "sv", label: "Svenska" },
    { code: "auto", label: "Identifiera automatiskt" },
    { code: "en", label: "Engelska" },
    { code: "no", label: "Norska" },
    { code: "da", label: "Danska" },
    { code: "fi", label: "Finska" },
  ];

  // ---- Setup state ----
  let models = $state<WhisperModel[]>([]);
  let selectedModel = $state("kb-whisper-small");
  let language = $state("sv");
  let diarize = $state(true);
  let autoSpeakers = $state(true);
  let numSpeakers = $state(2);

  let audioPath = $state<string | null>(null);
  let audioName = $state<string | null>(null);

  // ---- Transcript / review state ----
  let transcript = $state<Transcript | null>(null);
  let speakerLabels = $state<Record<string, string>>({});
  let view = $state<"transcript" | "review">("transcript");

  let analysis = $state<AnalyzeResult | null>(null);
  let rejected = $state<Set<number>>(new Set());
  let selectedProfile = $state("skola");
  let enabled = $state<Record<string, boolean>>(profileMap("skola"));
  let terms = $state<string[]>([]);
  let termInput = $state("");
  let useAi = $state(false);
  let wordTimestamps = $state(false);

  // ---- Recording ----
  let recording = $state(false);
  let recElapsed = $state(0);
  let recCtx: AudioContext | null = null;
  let recStream: MediaStream | null = null;
  let recNode: ScriptProcessorNode | null = null;
  let recChunks: Float32Array[] = [];
  let recSampleRate = 16000;
  let recTimer: ReturnType<typeof setInterval> | null = null;

  // ---- Process state ----
  let busy = $state(false);
  let progressMsg = $state("");
  let downloading = $state<string | null>(null);
  let downloadPct = $state(0);
  let error = $state("");
  let toast = $state("");

  const selectedDownloaded = $derived(models.find((m) => m.id === selectedModel)?.downloaded ?? false);

  $effect(() => {
    refreshModels();
    const saved = localStorage.getItem("avskrift_terms");
    if (saved) terms = JSON.parse(saved);
  });

  $effect(() => {
    const p = listen<string>("avskrift:progress", (e) => (progressMsg = e.payload));
    const d = listen<{ id: string; downloaded: number; total: number }>("avskrift:download", (e) => {
      downloading = e.payload.id;
      downloadPct = e.payload.total > 0 ? Math.round((e.payload.downloaded / e.payload.total) * 100) : 0;
    });
    return () => {
      p.then((f) => f());
      d.then((f) => f());
    };
  });

  async function refreshModels() {
    try {
      models = await invoke<WhisperModel[]>("list_whisper_models");
    } catch (e) {
      error = String(e);
    }
  }

  function showToast(msg: string) {
    toast = msg;
    setTimeout(() => (toast = ""), 2200);
  }

  async function openAudio() {
    const selected = await open({
      multiple: false,
      filters: [{ name: "Ljud", extensions: ["mp3", "wav", "m4a", "ogg", "flac", "webm", "mp4", "aac"] }],
    });
    if (typeof selected === "string") {
      audioPath = selected;
      audioName = selected.split(/[\\/]/).pop() ?? selected;
      transcript = null;
      analysis = null;
    }
  }

  async function downloadModel(id: string) {
    error = "";
    downloading = id;
    downloadPct = 0;
    try {
      await invoke("download_whisper_model", { id });
      await refreshModels();
      showToast("Modellen hämtades");
    } catch (e) {
      error = String(e);
    } finally {
      downloading = null;
    }
  }

  async function runTranscribe() {
    if (!audioPath || busy) return;
    busy = true;
    error = "";
    progressMsg = "Startar…";
    try {
      const t = await invoke<Transcript>("transcribe", {
        args: {
          path: audioPath,
          model: selectedModel,
          language,
          diarize,
          numSpeakers: diarize && !autoSpeakers ? numSpeakers : null,
          wordTimestamps,
        },
      });
      transcript = t;
      // Default speaker labels: "Talare 1" etc.
      const labels: Record<string, string> = {};
      for (const u of t.utterances) {
        if (u.speaker && !(u.speaker in labels)) {
          labels[u.speaker] = u.speaker.replace("TALARE_", "Talare ");
        }
      }
      speakerLabels = labels;
      analysis = null;
      view = "transcript";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      progressMsg = "";
    }
  }

  async function runAnonymize() {
    if (!transcript || busy) return;
    busy = true;
    error = "";
    progressMsg = "Avidentifierar…";
    try {
      const texts = transcript.utterances.map((u) => u.text);
      analysis = await invoke<AnalyzeResult>("anonymize", {
        args: { texts, enabled: ALL_KEYS, terms, useAi },
      });
      rejected = new Set();
      view = "review";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      progressMsg = "";
    }
  }

  function applyProfile(id: string) {
    selectedProfile = id;
    const next = profileMap(id);
    for (const k of ALL_KEYS) enabled[k] = next[k];
  }

  function isActive(id: number): boolean {
    if (!analysis) return false;
    const span = analysis.spans[id];
    return enabled[span.category] && !rejected.has(id);
  }
  const activeCount = $derived(analysis ? analysis.spans.filter((s) => isActive(s.id)).length : 0);
  function countFor(key: string): number {
    return analysis ? analysis.spans.filter((s) => s.category === key).length : 0;
  }
  function toggleSpan(id: number) {
    if (!analysis || !enabled[analysis.spans[id].category]) return;
    const next = new Set(rejected);
    next.has(id) ? next.delete(id) : next.add(id);
    rejected = next;
  }
  function rejectedIds(): number[] {
    return analysis ? analysis.spans.filter((s) => !isActive(s.id)).map((s) => s.id) : [];
  }

  function addTerms() {
    const incoming = termInput.split(/[\n,;]+/).map((t) => t.trim()).filter(Boolean);
    if (!incoming.length) return;
    terms = [...new Set([...terms, ...incoming])].sort((a, b) => a.localeCompare(b, "sv"));
    localStorage.setItem("avskrift_terms", JSON.stringify(terms));
    termInput = "";
    if (analysis) runAnonymize();
  }
  function removeTerm(t: string) {
    terms = terms.filter((x) => x !== t);
    localStorage.setItem("avskrift_terms", JSON.stringify(terms));
    if (analysis) runAnonymize();
  }

  const fileStem = $derived((audioName ?? "transkript").replace(/\.[^.]+$/, ""));
  const hasWords = $derived(!!transcript?.utterances.some((u) => u.words && u.words.length > 0));

  async function exportAs(ext: "txt" | "srt" | "vtt" | "docx", anonymize: boolean, wordLevel = false) {
    if (!transcript) return;
    const suffix = anonymize ? "_avidentifierad" : wordLevel ? "_ord" : "";
    const path = await save({
      defaultPath: `${fileStem}${suffix}.${ext}`,
      filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
    });
    if (!path) return;
    try {
      await invoke("export_transcript", {
        args: { path, anonymize, rejected: anonymize ? rejectedIds() : [], speakerLabels, wordLevel },
      });
      showToast("Filen sparades");
    } catch (e) {
      error = String(e);
    }
  }

  async function copyAnon() {
    if (!analysis || !transcript) return;
    try {
      const segs = await invoke<string[]>("anonymized_segments", { rejected: rejectedIds() });
      const text = transcript.utterances
        .map((u, i) => {
          const body = segs[i] ?? u.text;
          const name = u.speaker ? speakerLabels[u.speaker] ?? u.speaker : null;
          return name ? `${name}: ${body}` : body;
        })
        .join("\n");
      await navigator.clipboard.writeText(text);
      showToast("Kopierat till urklipp");
    } catch (e) {
      error = String(e);
    }
  }

  function fmtTime(s: number): string {
    const m = Math.floor(s / 60), sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, "0")}`;
  }

  // Group consecutive utterances by the same speaker for a cleaner transcript view.
  const groups = $derived.by(() => {
    if (!transcript) return [] as { speaker: string | null; start: number; texts: string[] }[];
    const out: { speaker: string | null; start: number; texts: string[] }[] = [];
    for (const u of transcript.utterances) {
      const last = out[out.length - 1];
      if (last && last.speaker === u.speaker) last.texts.push(u.text);
      else out.push({ speaker: u.speaker, start: u.start, texts: [u.text] });
    }
    return out;
  });
</script>

<div class="app">
  <header>
    <svg class="logo" viewBox="0 0 48 48" fill="none" aria-hidden="true">
      <rect x="9" y="10" width="20" height="28" rx="2" fill="#fff" stroke="#111214" stroke-width="2" />
      <rect x="13" y="17" width="12" height="2.6" fill="#111214" />
      <rect x="13" y="22" width="12" height="2.6" fill="#2440ff" />
      <rect x="13" y="27" width="7" height="2.6" fill="#c9ccd2" />
      <path d="M34 16v16M38 20v8M30 21v6" stroke="#2440ff" stroke-width="2.4" stroke-linecap="round" />
    </svg>
    <div class="brand">
      <h1>Avskrift</h1>
      <p>Transkribering, talaridentifiering och avidentifiering — helt lokalt</p>
    </div>
    <div class="spacer"></div>
    <div class="lockbadge"><span class="dot"></span> Allt körs lokalt</div>
  </header>

  <div class="layout">
    <aside class="sidebar">
      <section>
        <h2>Ljudfil</h2>
        {#if recording}
          <div class="recording">
            <span class="recdot"></span> Spelar in… {fmtTime(recElapsed)}
            <button class="btn block" onclick={stopRecording}>Stoppa inspelning</button>
          </div>
        {:else if audioName}
          <div class="file-chip">
            <span title={audioPath}>{audioName}</span>
            <button class="link" onclick={() => { audioPath = null; audioName = null; transcript = null; analysis = null; }}>rensa</button>
          </div>
        {:else}
          <button class="btn block" onclick={openAudio}>Välj ljudfil…</button>
          <button class="btn block mt" onclick={startRecording}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/></svg>
            Spela in
          </button>
          <p class="hint">mp3, wav, m4a, ogg, flac … eller spela in direkt</p>
        {/if}
      </section>

      <section>
        <h2>Modell</h2>
        <select class="profile" bind:value={selectedModel}>
          {#each models as m (m.id)}
            <option value={m.id}>{m.label}{m.downloaded ? "" : " — hämtas"}</option>
          {/each}
        </select>
        {#if !selectedDownloaded}
          {#if downloading === selectedModel}
            <div class="dl"><div class="dl-bar" style="width:{downloadPct}%"></div></div>
            <p class="hint">Hämtar… {downloadPct}%</p>
          {:else}
            <button class="btn block" onclick={() => downloadModel(selectedModel)} disabled={!!downloading}>
              Hämta modell ({models.find((m) => m.id === selectedModel)?.sizeMb ?? "?"} MB)
            </button>
          {/if}
        {/if}
      </section>

      <section>
        <h2>Språk</h2>
        <select class="profile" bind:value={language}>
          {#each LANGUAGES as l (l.code)}<option value={l.code}>{l.label}</option>{/each}
        </select>
      </section>

      <section>
        <h2>Talare</h2>
        <label class="ai-toggle">
          <input type="checkbox" bind:checked={diarize} />
          <span>Identifiera talare (diarisering)<em>delar upp transkriptet per röst</em></span>
        </label>
        {#if diarize}
          <label class="ai-toggle">
            <input type="checkbox" bind:checked={autoSpeakers} />
            <span>Räkna ut antal automatiskt</span>
          </label>
          {#if !autoSpeakers}
            <label class="numrow">Antal talare
              <input type="number" min="1" max="10" bind:value={numSpeakers} />
            </label>
          {/if}
        {/if}
        <label class="ai-toggle">
          <input type="checkbox" bind:checked={wordTimestamps} />
          <span>Ordnivå-tidsstämplar<em>tid per ord — för exakta undertexter (.vtt)</em></span>
        </label>
      </section>

      <button class="btn primary block big" onclick={runTranscribe}
        disabled={busy || !audioPath || !selectedDownloaded}>
        {busy ? "Arbetar…" : "Transkribera"}
      </button>

      {#if transcript}
        <section class="anon-block">
          <h2>Avidentifiering</h2>
          <select class="profile" value={selectedProfile} onchange={(e) => applyProfile(e.currentTarget.value)}>
            {#each PROFILES as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
          </select>
          <label class="ai-toggle">
            <input type="checkbox" bind:checked={useAi} />
            <span>Djupare granskning (AI)<em>långsammare, fångar fler ledtrådar</em></span>
          </label>
          <button class="btn block" onclick={runAnonymize} disabled={busy}>
            {analysis ? "Kör om avidentifiering" : "Avidentifiera transkript"}
          </button>
        </section>

        {#if analysis}
          <section>
            <h2>Kategorier</h2>
            <ul class="filters">
              {#each CATEGORIES as cat (cat.key)}
                <li>
                  <label><input type="checkbox" bind:checked={enabled[cat.key]} />
                    <span class="dotc" style="background:{cat.color}"></span>{cat.label}</label>
                  <span class="count">{countFor(cat.key)}</span>
                </li>
              {/each}
            </ul>
          </section>
          <section>
            <h2>Egen ordlista</h2>
            <textarea bind:value={termInput} placeholder="Ord att alltid maska. Ett per rad." rows="2"></textarea>
            <button class="btn block" onclick={addTerms} disabled={termInput.trim() === ""}>Lägg till</button>
            {#if terms.length}
              <ul class="terms">
                {#each terms as t (t)}<li><span>{t}</span><button class="x" onclick={() => removeTerm(t)} aria-label="Ta bort">×</button></li>{/each}
              </ul>
            {/if}
          </section>
        {/if}
      {/if}
    </aside>

    <main class="review">
      {#if error}<div class="banner error">{error}</div>{/if}
      {#if analysis?.warnings.length}{#each analysis.warnings as w}<div class="banner warn">{w}</div>{/each}{/if}

      {#if busy}
        <div class="state">
          <div class="spinner"></div>
          <p class="state-title">{progressMsg || "Arbetar…"}</p>
          <p class="state-sub">Allt körs lokalt på din dator. Första körningen laddar modellen.</p>
        </div>
      {:else if !transcript}
        <div class="state">
          <svg class="state-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M12 3v18M8 7v10M16 7v10M4 10v4M20 10v4"/></svg>
          <p class="state-title">Välj en ljudfil och transkribera</p>
          <p class="state-sub">Välj modell och språk, slå på <strong>diarisering</strong> för att skilja talare åt, och klicka <strong>Transkribera</strong>. Sedan kan du avidentifiera och exportera.</p>
        </div>
      {:else}
        <div class="review-head">
          <div class="tabs">
            <button class="tab" class:on={view === "transcript"} onclick={() => (view = "transcript")}>Transkript</button>
            <button class="tab" class:on={view === "review"} onclick={() => (view = "review")} disabled={!analysis}>Avidentifiering</button>
          </div>
          <div class="actions">
            {#if view === "review" && analysis}
              <button class="btn primary" onclick={copyAnon}>Kopiera</button>
              <button class="btn" onclick={() => exportAs("txt", true)}>.txt</button>
              <button class="btn" onclick={() => exportAs("docx", true)}>Word</button>
              <button class="btn" onclick={() => exportAs("srt", true)}>.srt</button>
              <button class="btn" onclick={() => exportAs("vtt", true)}>.vtt</button>
            {:else}
              <button class="btn" onclick={() => exportAs("txt", false)}>.txt</button>
              <button class="btn" onclick={() => exportAs("docx", false)}>Word</button>
              <button class="btn" onclick={() => exportAs("srt", false)}>.srt</button>
              <button class="btn" onclick={() => exportAs("vtt", false)}>.vtt</button>
              {#if hasWords}
                <button class="btn" onclick={() => exportAs("vtt", false, true)} title="En undertext per ord">.vtt (ord)</button>
              {/if}
            {/if}
          </div>
        </div>

        {#if view === "transcript"}
          <div class="meta">
            {transcript.utterances.length} segment · modell {transcript.model}{transcript.diarized ? " · diariserad" : ""}
          </div>
          <div class="transcript">
            {#each groups as g}
              <div class="turn">
                {#if g.speaker}
                  <input class="speaker" bind:value={speakerLabels[g.speaker]} />
                {/if}
                <p class="utext"><span class="ts">{fmtTime(g.start)}</span>{g.texts.join(" ")}</p>
              </div>
            {/each}
          </div>
        {:else if analysis}
          <div class="meta"><strong>{activeCount}</strong> av {analysis.spans.length} träffar avidentifieras</div>
          <div class="document">{#each analysis.segments as seg}{#if seg.span === null}{seg.text}{:else}{@const info = analysis.spans[seg.span]}{@const active = isActive(seg.span)}{@const off = !enabled[info.category]}<button
                  class="hit" class:active class:rejected={!active && !off} class:disabled={off}
                  style="--c:{colorOf(info.category)}"
                  title={active ? `${info.text} → ${info.replacement}` : info.text}
                  onclick={() => toggleSpan(seg.span!)}>{seg.text}</button>{/if}{/each}</div>
          <div class="reassure">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 3l8 4v5c0 5-3.4 7.7-8 9-4.6-1.3-8-4-8-9V7l8-4z"/><path d="M9 12l2 2 4-4"/></svg>
            Granska alltid träffarna innan du delar. Ingen automatik fångar 100 %.
          </div>
        {/if}
      {/if}
    </main>
  </div>
</div>

{#if toast}
  <div class="toast"><span class="accentbar"></span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6"><path d="M5 13l4 4L19 7"/></svg>{toast}</div>
{/if}

<style>
  :global(:root) {
    --ink: #111214; --muted: #6a6e74; --faint: #a2a6ac; --bg: #ffffff;
    --line: #ececec; --line-2: #e2e2e2; --accent: #2440ff;
  }
  :global(body) { margin: 0; font-family: "Archivo", system-ui, sans-serif; color: var(--ink); background: var(--bg); -webkit-font-smoothing: antialiased; }
  .app { height: 100vh; display: flex; flex-direction: column; }

  header { display: flex; align-items: flex-end; gap: 15px; padding: 20px 30px 16px; border-bottom: 1px solid var(--line); }
  .logo { width: 36px; height: 36px; flex: none; margin-bottom: 3px; }
  .brand h1 { font-family: "Instrument Serif", serif; font-weight: 400; font-size: 34px; line-height: .9; margin: 0; }
  .brand p { margin: 5px 0 0; font-size: 13px; color: var(--muted); }
  .spacer { flex: 1; }
  .lockbadge { display: inline-flex; align-items: center; gap: 8px; font-size: 11.5px; letter-spacing: .05em; text-transform: uppercase; color: var(--muted); margin-bottom: 5px; }
  .dot { width: 6px; height: 6px; border-radius: 50%; background: #16a34a; box-shadow: 0 0 0 3px rgba(22,163,74,.15); }

  .layout { flex: 1; display: grid; grid-template-columns: 310px 1fr; overflow: hidden; }
  .sidebar { padding: 22px 24px; overflow: auto; border-right: 1px solid var(--line); }
  section { margin-bottom: 22px; }
  h2 { font-size: 11px; letter-spacing: .15em; text-transform: uppercase; color: var(--faint); margin: 0 0 11px; font-weight: 600; }
  .hint { font-size: 12px; color: var(--faint); margin: 6px 0 0; }

  textarea, select.profile {
    width: 100%; box-sizing: border-box; font: inherit; font-size: 14px; color: var(--ink);
    border: 1px solid var(--line-2); border-radius: 3px; padding: 10px 12px; background: var(--bg);
  }
  textarea { resize: vertical; }
  select.profile { appearance: none; cursor: pointer;
    background-image: linear-gradient(45deg, transparent 50%, var(--ink) 50%), linear-gradient(135deg, var(--ink) 50%, transparent 50%);
    background-position: calc(100% - 18px) 18px, calc(100% - 13px) 18px; background-size: 5px 5px, 5px 5px; background-repeat: no-repeat; }

  .btn { font: inherit; font-size: 13.5px; font-weight: 500; border: 1px solid var(--ink); background: var(--bg); color: var(--ink); border-radius: 3px; padding: 9px 14px; cursor: pointer; transition: .14s; display: inline-flex; align-items: center; justify-content: center; gap: 7px; }
  .btn:hover:not(:disabled) { background: var(--ink); color: #fff; }
  .btn:disabled { opacity: .4; cursor: default; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .btn.primary:hover:not(:disabled) { filter: brightness(1.12); }
  .btn.block { width: 100%; }
  .btn.mt { margin-top: 8px; }
  .btn.big { padding: 12px; font-size: 15px; margin-bottom: 22px; }
  .link { border: none; background: none; color: var(--accent); cursor: pointer; font: inherit; font-size: 13px; padding: 0 2px; }
  .x { border: none; background: none; color: var(--muted); cursor: pointer; font-size: 16px; line-height: 1; padding: 0 2px; }

  .file-chip { display: flex; justify-content: space-between; align-items: center; gap: 8px; background: #f6f7ff; border: 1px solid #dfe3ff; border-radius: 3px; padding: 9px 11px; font-size: 13.5px; }
  .file-chip span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .recording { font-size: 13.5px; color: var(--ink); display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .recording .btn { margin-top: 8px; }
  .recdot { width: 9px; height: 9px; border-radius: 50%; background: #e11d48; animation: pulse 1.1s ease-in-out infinite; }
  @keyframes pulse { 0%,100% { opacity: 1; } 50% { opacity: .3; } }

  .ai-toggle { display: flex; align-items: flex-start; gap: 9px; margin-top: 10px; font-size: 13px; color: var(--muted); }
  .ai-toggle em { font-style: normal; color: var(--faint); display: block; margin-top: 2px; font-size: 12px; }
  .numrow { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-top: 10px; font-size: 13.5px; }
  .numrow input { width: 64px; font: inherit; padding: 6px 8px; border: 1px solid var(--line-2); border-radius: 3px; }
  input[type="checkbox"] { width: 15px; height: 15px; accent-color: var(--ink); }

  .dl { height: 6px; background: var(--line); border-radius: 3px; margin-top: 9px; overflow: hidden; }
  .dl-bar { height: 100%; background: var(--accent); transition: width .2s; }

  .anon-block { border-top: 1px solid var(--line); padding-top: 18px; }
  .filters { list-style: none; margin: 0; padding: 0; }
  .filters li { display: flex; align-items: center; justify-content: space-between; padding: 6px 0; border-bottom: 1px solid var(--line); }
  .filters li:last-child { border-bottom: none; }
  .filters label { display: flex; align-items: center; gap: 10px; font-size: 13.5px; cursor: pointer; }
  .dotc { width: 9px; height: 9px; border-radius: 50%; flex: none; }
  .count { font-size: 12px; color: var(--faint); font-variant-numeric: tabular-nums; }
  .terms { list-style: none; margin: 10px 0 0; padding: 0; display: flex; flex-wrap: wrap; gap: 6px; }
  .terms li { display: flex; align-items: center; gap: 4px; border: 1px solid var(--line-2); border-radius: 3px; padding: 3px 4px 3px 9px; font-size: 12.5px; }

  .review { padding: 22px 30px; display: flex; flex-direction: column; overflow: hidden; }
  .review-head { display: flex; align-items: center; justify-content: space-between; gap: 16px; margin-bottom: 14px; }
  .tabs { display: flex; gap: 4px; }
  .tab { font: inherit; font-size: 14px; font-weight: 600; border: none; background: none; color: var(--faint); cursor: pointer; padding: 6px 4px; border-bottom: 2px solid transparent; }
  .tab.on { color: var(--ink); border-bottom-color: var(--accent); }
  .tab:disabled { opacity: .4; cursor: default; }
  .actions { display: flex; gap: 7px; flex-wrap: wrap; }
  .meta { font-size: 13px; color: var(--muted); margin-bottom: 12px; }
  .meta strong { color: var(--ink); font-weight: 700; }

  .transcript { flex: 1; overflow: auto; max-width: 78ch; }
  .turn { margin-bottom: 16px; }
  .speaker { font: inherit; font-weight: 700; font-size: 13px; color: var(--accent); border: none; background: none; padding: 0 0 2px; border-bottom: 1px dashed transparent; }
  .speaker:hover, .speaker:focus { border-bottom-color: var(--line-2); outline: none; }
  .utext { margin: 3px 0 0; line-height: 1.7; font-size: 15.5px; }
  .ts { font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; margin-right: 10px; }

  .document { flex: 1; overflow: auto; white-space: pre-wrap; line-height: 2.1; font-size: 16px; max-width: 76ch; padding: 4px 2px; }
  .hit { border: none; background: none; font: inherit; line-height: inherit; cursor: pointer; padding: 0 1px 1px; border-bottom: 2px solid var(--c); transition: background .14s; color: inherit; }
  .hit:hover { background: color-mix(in srgb, var(--c) 13%, transparent); }
  .hit.rejected { border-bottom: 2px dotted var(--faint); text-decoration: line-through; color: var(--faint); }
  .hit.disabled { border-bottom: none; cursor: default; }

  .reassure { margin-top: 16px; padding-top: 14px; border-top: 1px solid var(--line); font-size: 12.5px; color: var(--muted); display: flex; align-items: center; gap: 9px; }
  .reassure svg { width: 15px; height: 15px; color: var(--accent); }

  .state { margin: auto; text-align: center; color: var(--muted); max-width: 400px; padding: 30px; }
  .state-icon { width: 40px; height: 40px; color: var(--line-2); margin-bottom: 14px; }
  .state-title { font-family: "Instrument Serif", serif; font-size: 24px; color: var(--ink); margin: 0 0 6px; }
  .state-sub { font-size: 14px; margin: 0; line-height: 1.6; }
  .spinner { width: 34px; height: 34px; border: 3px solid var(--line); border-top-color: var(--accent); border-radius: 50%; margin: 0 auto 16px; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .banner { border-radius: 3px; padding: 10px 13px; margin-bottom: 12px; font-size: 13.5px; }
  .banner.error { background: #fef2f2; color: #b91c1c; border: 1px solid #fecaca; }
  .banner.warn { background: #fffbeb; color: #92400e; border: 1px solid #fde68a; }

  .toast { position: fixed; bottom: 28px; left: 50%; transform: translateX(-50%); background: var(--ink); color: #fff; padding: 12px 18px 12px 20px; border-radius: 3px; font-size: 13.5px; font-weight: 500; display: flex; align-items: center; gap: 9px; box-shadow: 0 18px 44px rgba(0,0,0,.28); overflow: hidden; }
  .toast svg { width: 16px; height: 16px; color: var(--accent); }
  .toast .accentbar { position: absolute; left: 0; top: 0; bottom: 0; width: 3px; background: var(--accent); }
</style>
