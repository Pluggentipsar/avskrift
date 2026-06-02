<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open, save, ask } from "@tauri-apps/plugin-dialog";
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
  type Segment = { text: string; span: number | null; start: number; end: number; word: boolean };
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
  let translate = $state(false);

  let audioPath = $state<string | null>(null);
  let audioName = $state<string | null>(null);

  // ---- Transcript / review state ----
  let transcript = $state<Transcript | null>(null);
  let speakerLabels = $state<Record<string, string>>({});
  let view = $state<"transcript" | "review" | "summary" | "qa">("transcript");

  // ---- Summarisation ----
  type SummaryModel = { id: string; label: string; sizeMb: number; downloaded: boolean };
  type TemplateInfo = { id: string; label: string };
  let summaryModels = $state<SummaryModel[]>([]);
  let summaryTemplates = $state<TemplateInfo[]>([]);
  let selectedSummaryModel = $state("qwen2.5-3b");
  let selectedTemplate = $state("protokoll");
  let customHeadings = $state("## Närvarande\n## Dagordning\n## Beslut\n## Åtgärder");
  let summaryFromAnon = $state(false);
  let summaryDraft = $state("");
  const summaryDownloaded = $derived(summaryModels.find((m) => m.id === selectedSummaryModel)?.downloaded ?? false);

  // ---- Editing, corrections, projects, export options ----
  let editingIdx = $state<number | null>(null);
  let editText = $state("");
  let dirty = $state(false); // unsaved edits since last transcribe/open
  let correctionInput = $state("");
  let exportTimestamps = $state(false);
  let includeTranscript = $state(false);
  let transcribePct = $state<number | null>(null);

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

  // ---- Meeting capture (dual-stream: mic = "Jag", system loopback = "Mötet") ----
  let meetingActive = $state(false);
  let meetingElapsed = $state(0);
  let meetingBusy = $state(false);
  let meetingTimer: ReturnType<typeof setInterval> | null = null;
  let meetingSysWav = $state<string | null>(null);
  let meetingMicWav = $state<string | null>(null);
  // Live transcription feed (filled by avskrift:meeting-utterance events during a meeting).
  let liveUtterances = $state<{ source: string; start: number; end: number; text: string }[]>([]);
  let liveFeedEl = $state<HTMLDivElement | null>(null);
  let meetingLive = $state(true); // transcribe live during the meeting (vs only after stop)
  let meetingLagging = $state(false); // worker fell behind real time (weak hardware)

  // ---- Meeting Q&A ("Fråga mötet") — works on any transcript ----
  let qaQuestion = $state("");
  let qaHistory = $state<{ q: string; a: string }[]>([]);
  let qaBusy = $state(false);

  // ---- Playback (synced with the transcript) ----
  let audioEl = $state<HTMLAudioElement | null>(null);
  let playing = $state(false);
  let currentTime = $state(0);
  // Asset-protocol URL so the webview can stream the local file (see tauri.conf assetProtocol).
  const audioSrc = $derived(audioPath ? convertFileSrc(audioPath) : "");

  function seekTo(t: number) {
    if (!audioEl) return;
    const target = Math.max(0, t);
    const apply = () => {
      audioEl!.currentTime = target;
      void audioEl!.play();
    };
    // Setting currentTime before the media metadata has loaded is silently ignored by the
    // webview, so a timestamp click from a fresh (never-played) state wouldn't seek. Defer
    // until the audio knows its duration.
    if (audioEl.readyState >= 1 /* HAVE_METADATA */) {
      apply();
    } else {
      audioEl.addEventListener("loadedmetadata", apply, { once: true });
      audioEl.load();
    }
  }
  function togglePlay() {
    if (!audioEl) return;
    audioEl.paused ? audioEl.play() : audioEl.pause();
  }

  // Index of the utterance currently playing (for highlight), or -1.
  const activeUtterance = $derived.by(() => {
    if (!transcript || !playing) return -1;
    return transcript.utterances.findIndex((u) => currentTime >= u.start && currentTime < u.end);
  });

  // Keep the currently playing word/segment scrolled into view during playback.
  $effect(() => {
    if (!playing || view !== "transcript") return;
    void currentTime; // re-run as playback advances
    const el = document.querySelector(".word.playing, .useg.playing");
    el?.scrollIntoView({ block: "center", behavior: "smooth" });
  });

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
    refreshSummaryModels();
    refreshJobs();
    invoke<TemplateInfo[]>("list_summary_templates").then((t) => (summaryTemplates = t)).catch(() => {});
    const saved = localStorage.getItem("avskrift_terms");
    if (saved) terms = JSON.parse(saved);
    // Pick a sensible default Whisper model + meeting mode for this machine (user can override).
    const savedModel = localStorage.getItem("avskrift_model");
    selectedModel = savedModel || hwDefaultModel();
    meetingLive = !isWeakHardware();
  });

  // Remember the chosen Whisper model across sessions.
  $effect(() => { localStorage.setItem("avskrift_model", selectedModel); });

  $effect(() => {
    const p = listen<string>("avskrift:progress", (e) => (progressMsg = e.payload));
    const d = listen<{ id: string; downloaded: number; total: number }>("avskrift:download", (e) => {
      downloading = e.payload.id;
      downloadPct = e.payload.total > 0 ? Math.round((e.payload.downloaded / e.payload.total) * 100) : 0;
    });
    const pc = listen<number>("avskrift:percent", (e) => (transcribePct = e.payload));
    const mu = listen<{ source: string; start: number; end: number; text: string }>(
      "avskrift:meeting-utterance",
      (e) => { liveUtterances = [...liveUtterances, e.payload]; },
    );
    const ml = listen<boolean>("avskrift:meeting-lag", () => (meetingLagging = true));
    return () => {
      p.then((f) => f());
      d.then((f) => f());
      pc.then((f) => f());
      mu.then((f) => f());
      ml.then((f) => f());
    };
  });

  // Auto-scroll the live meeting feed to the latest line.
  $effect(() => {
    liveUtterances.length;
    if (liveFeedEl) liveFeedEl.scrollTop = liveFeedEl.scrollHeight;
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

  async function refreshSummaryModels() {
    try {
      summaryModels = await invoke<SummaryModel[]>("list_summary_models");
    } catch (e) {
      error = String(e);
    }
  }

  async function downloadSummaryModel(id: string) {
    error = "";
    downloading = id;
    downloadPct = 0;
    try {
      await invoke("download_summary_model", { id });
      await refreshSummaryModels();
      showToast("Modellen hämtades");
    } catch (e) {
      error = String(e);
    } finally {
      downloading = null;
    }
  }

  /** Build the transcript text to feed the summariser: speaker-prefixed lines, raw or anonymised. */
  async function summaryInputText(): Promise<string> {
    if (!transcript) return "";
    let bodies = transcript.utterances.map((u) => u.text);
    if (summaryFromAnon && analysis) {
      try {
        bodies = await invoke<string[]>("anonymized_segments", { rejected: rejectedIds() });
      } catch (e) {
        error = String(e);
      }
    }
    return transcript.utterances
      .map((u, i) => {
        const name = u.speaker ? speakerLabels[u.speaker] ?? u.speaker : null;
        const body = bodies[i] ?? u.text;
        return name ? `${name}: ${body}` : body;
      })
      .join("\n");
  }

  async function runSummarize() {
    if (!transcript || busy) return;
    if (!summaryDownloaded) {
      error = "Hämta den valda sammanfattningsmodellen först.";
      return;
    }
    busy = true;
    error = "";
    progressMsg = "Förbereder…";
    try {
      const text = await summaryInputText();
      summaryDraft = await invoke<string>("summarize", {
        args: { text, model: selectedSummaryModel, template: selectedTemplate, customHeadings },
      });
      view = "summary";
      await saveCurrentJob("transcribe");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      progressMsg = "";
    }
  }

  async function copySummary() {
    if (!summaryDraft) return;
    await navigator.clipboard.writeText(summaryDraft);
    showToast("Kopierat till urklipp");
  }

  async function saveSummary(ext: "txt" | "docx") {
    if (!summaryDraft) return;
    const path = await save({
      defaultPath: `${fileStem}_sammanfattning.${ext}`,
      filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
    });
    if (!path) return;
    try {
      await invoke("save_summary", {
        args: { path, text: summaryDraft, includeTranscript, timestamps: exportTimestamps, speakerLabels },
      });
      showToast("Filen sparades");
    } catch (e) {
      error = String(e);
    }
  }

  async function runTranscribe() {
    if (!audioPath || busy) return;
    busy = true;
    error = "";
    transcribePct = 0;
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
          translate,
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
      summaryDraft = "";
      meetingSysWav = null;
      dirty = false;
      view = "transcript";
      currentJobId = null; // a fresh transcription starts a new history entry
      currentJobCreatedAt = null;
      await saveCurrentJob("transcribe");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      transcribePct = null;
      progressMsg = "";
    }
  }

  // ---- Transcript editing ----

  function startEdit(idx: number) {
    if (!transcript) return;
    editingIdx = idx;
    editText = transcript.utterances[idx].text;
  }

  async function commitEdit() {
    if (editingIdx === null || !transcript) return;
    const idx = editingIdx;
    const next = editText.trim();
    editingIdx = null;
    if (next === transcript.utterances[idx].text) return;
    transcript.utterances[idx].text = next;
    // Editing invalidates word-level timings for that utterance; drop them to avoid stale highlights.
    transcript.utterances[idx].words = [];
    await pushTranscript();
  }

  function cancelEdit() {
    editingIdx = null;
  }

  /** Persist the (edited) transcript to the backend so anonymise/summarise/export use it. */
  async function pushTranscript() {
    if (!transcript) return;
    dirty = true;
    analysis = null; // any prior anonymisation no longer matches the edited text
    try {
      await invoke("update_transcript", { transcript });
    } catch (e) {
      error = String(e);
    }
  }

  async function renameSpeaker(id: string, name: string) {
    speakerLabels[id] = name;
    // Labels live in the UI; nothing to push to the transcript itself.
  }

  /** Apply find→replace corrections (one "fel=>rätt" per line) across every utterance. */
  async function applyCorrections() {
    if (!transcript) return;
    const rules = correctionInput
      .split("\n")
      .map((l) => l.split(/=>|->|=/).map((s) => s.trim()))
      .filter((p) => p.length >= 2 && p[0].length > 0);
    if (!rules.length) return;
    let changed = 0;
    for (const u of transcript.utterances) {
      let t = u.text;
      for (const [from, to] of rules) {
        // Whole-word, case-insensitive, escaped.
        const re = new RegExp(`\\b${from.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}\\b`, "gi");
        t = t.replace(re, to);
      }
      if (t !== u.text) {
        u.text = t;
        u.words = [];
        changed++;
      }
    }
    if (changed) {
      await pushTranscript();
      showToast(`Rättade ${changed} segment`);
    } else {
      showToast("Inga träffar");
    }
  }

  // ---- Projects ----

  async function saveProject() {
    if (!transcript) return;
    const path = await save({
      defaultPath: `${fileStem}.avskrift`,
      filters: [{ name: "Avskrift-projekt", extensions: ["avskrift"] }],
    });
    if (!path) return;
    try {
      await invoke("save_project", { args: { path, speakerLabels, audioPath } });
      dirty = false;
      showToast("Projektet sparades");
    } catch (e) {
      error = String(e);
    }
  }

  async function openProject() {
    const path = await open({ multiple: false, filters: [{ name: "Avskrift-projekt", extensions: ["avskrift"] }] });
    if (typeof path !== "string") return;
    try {
      const p = await invoke<{ transcript: Transcript; speakerLabels: Record<string, string>; audioPath: string | null }>(
        "open_project",
        { path },
      );
      transcript = p.transcript;
      speakerLabels = p.speakerLabels ?? {};
      audioPath = p.audioPath ?? null;
      audioName = audioPath ? audioPath.split(/[\\/]/).pop() ?? audioPath : "projekt";
      analysis = null;
      summaryDraft = "";
      dirty = false;
      view = "transcript";
      showToast("Projektet öppnades");
    } catch (e) {
      error = String(e);
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

  // ---- Manual masking: click an undetected word in the review to mask it ----
  let maskTarget = $state<{ start: number; end: number; text: string } | null>(null);
  let maskCategory = $state("ovrigt");
  let maskCustom = $state("");
  function openMask(seg: { start: number; end: number; text: string }) {
    maskTarget = { start: seg.start, end: seg.end, text: seg.text };
    maskCategory = "ovrigt";
    maskCustom = "";
  }
  async function confirmMask() {
    if (!maskTarget) return;
    try {
      analysis = await invoke<AnalyzeResult>("add_manual_span", {
        args: {
          start: maskTarget.start,
          end: maskTarget.end,
          category: maskCategory,
          custom: maskCustom.trim() || null,
        },
      });
      rejected = new Set(); // span ids were renumbered → reset (safe: defaults to fully masked)
      maskTarget = null;
    } catch (e) {
      error = String(e);
    }
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
        args: {
          path,
          anonymize,
          rejected: anonymize ? rejectedIds() : [],
          speakerLabels,
          wordLevel,
          timestamps: exportTimestamps,
        },
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

  async function startRecording() {
    if (recording) return;
    error = "";
    try {
      recStream = await navigator.mediaDevices.getUserMedia({ audio: true });
      recCtx = new AudioContext();
      recSampleRate = recCtx.sampleRate;
      const source = recCtx.createMediaStreamSource(recStream);
      recNode = recCtx.createScriptProcessor(4096, 1, 1);
      recChunks = [];
      recNode.onaudioprocess = (e) => {
        recChunks.push(new Float32Array(e.inputBuffer.getChannelData(0)));
      };
      source.connect(recNode);
      recNode.connect(recCtx.destination); // required for the processor to run
      recording = true;
      recElapsed = 0;
      recTimer = setInterval(() => (recElapsed += 1), 1000);
    } catch (e) {
      error = "Kunde inte komma åt mikrofonen: " + String(e);
    }
  }

  async function stopRecording() {
    if (!recording) return;
    recording = false;
    if (recTimer) clearInterval(recTimer);
    recNode?.disconnect();
    recStream?.getTracks().forEach((t) => t.stop());
    await recCtx?.close();

    // Flatten captured chunks, downsample to 16 kHz, encode a 16-bit PCM WAV (small IPC payload,
    // matches the pipeline's target rate).
    const total = recChunks.reduce((n, c) => n + c.length, 0);
    const pcm = new Float32Array(total);
    let off = 0;
    for (const c of recChunks) { pcm.set(c, off); off += c.length; }
    const down = downsampleTo16k(pcm, recSampleRate);
    const wav = encodeWav(down, 16000);
    recChunks = [];
    recCtx = recNode = recStream = null;

    try {
      const path = await invoke<string>("save_recording", { data: Array.from(new Uint8Array(wav)) });
      audioPath = path;
      audioName = `Inspelning (${fmtTime(recElapsed)})`;
      transcript = null;
      analysis = null;
      showToast("Inspelning klar");
    } catch (e) {
      error = String(e);
    }
  }

  // ---- Meeting capture (backend dual-stream WASAPI; see capture.rs) ----

  // Rough hardware tiering from the browser (cores + approx RAM) → default model & meeting mode.
  function isWeakHardware(): boolean {
    const cores = navigator.hardwareConcurrency || 8;
    const mem = (navigator as any).deviceMemory as number | undefined;
    return cores <= 4 || (!!mem && mem <= 4);
  }
  function hwDefaultModel(): string {
    const cores = navigator.hardwareConcurrency || 8;
    const mem = (navigator as any).deviceMemory as number | undefined;
    if (cores <= 2 || (!!mem && mem <= 2)) return "kb-whisper-tiny";
    if (isWeakHardware()) return "kb-whisper-base";
    return "kb-whisper-small";
  }

  async function startMeeting() {
    if (meetingActive || meetingBusy) return;
    error = "";
    try {
      meetingLagging = false;
      await invoke("start_meeting", { args: { model: selectedModel, language, live: meetingLive } });
      meetingActive = true;
      meetingElapsed = 0;
      liveUtterances = [];
      meetingTimer = setInterval(() => (meetingElapsed += 1), 1000);
    } catch (e) {
      error = "Kunde inte starta inspelningen: " + String(e);
    }
  }

  async function stopMeeting() {
    if (!meetingActive) return;
    meetingActive = false;
    if (meetingTimer) clearInterval(meetingTimer);
    meetingBusy = true;
    transcribePct = 0;
    progressMsg = "Avslutar inspelning…";
    try {
      const res = await invoke<any>("stop_meeting", {
        args: { model: selectedModel, language },
      });
      transcript = res.transcript;
      // Speaker ids are already "Jag" / "Mötet"; map each to itself so the rename UI lists them.
      const labels: Record<string, string> = {};
      for (const u of res.transcript.utterances) {
        if (u.speaker && !(u.speaker in labels)) labels[u.speaker] = u.speaker;
      }
      speakerLabels = labels;
      analysis = null;
      summaryDraft = "";
      dirty = false;
      meetingSysWav = res.systemWavPath ?? null;
      meetingMicWav = res.micWavPath ?? null;
      audioPath = res.systemWavPath ?? null;
      audioName = `Möte (${fmtTime(meetingElapsed)})`;
      view = "transcript";
      currentJobId = null;
      currentJobCreatedAt = null;
      screen = "transcribe";
      await saveCurrentJob("meeting");
      showToast("Möte transkriberat");
    } catch (e) {
      error = String(e);
    } finally {
      meetingBusy = false;
      transcribePct = null;
      progressMsg = "";
    }
  }

  /** Ask a free-text question about the current transcript (answer strictly from it). */
  async function askMeeting() {
    const q = qaQuestion.trim();
    if (!q || qaBusy || !transcript) return;
    qaBusy = true;
    error = "";
    progressMsg = "Tänker…";
    try {
      const text = await summaryInputText();
      const a = await invoke<string>("ask_transcript", {
        args: { question: q, transcriptText: text, model: selectedSummaryModel },
      });
      qaHistory = [...qaHistory, { q, a }];
      qaQuestion = "";
    } catch (e) {
      error = String(e);
    } finally {
      qaBusy = false;
      progressMsg = "";
    }
  }

  /** Split the meeting's "Mötet" utterances into distinct speakers via diarisation of the system WAV. */
  async function separateMeetingVoices() {
    if (!meetingSysWav || !transcript || busy) return;
    busy = true;
    error = "";
    progressMsg = "Separerar mötesröster…";
    try {
      const t = await invoke<Transcript>("diarize_meeting", {
        args: { systemWavPath: meetingSysWav, numSpeakers: null },
      });
      transcript = t;
      const labels: Record<string, string> = {};
      for (const u of t.utterances) {
        if (u.speaker && !(u.speaker in labels)) {
          labels[u.speaker] = u.speaker.startsWith("TALARE_") ? u.speaker.replace("TALARE_", "Talare ") : u.speaker;
        }
      }
      speakerLabels = labels;
      dirty = false;
      await saveCurrentJob("meeting");
      showToast("Mötesröster separerade");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      progressMsg = "";
    }
  }

  /** Simple linear-interpolation downsample to 16 kHz. */
  function downsampleTo16k(samples: Float32Array, srcRate: number): Float32Array {
    if (srcRate === 16000) return samples;
    const ratio = srcRate / 16000;
    const outLen = Math.floor(samples.length / ratio);
    const out = new Float32Array(outLen);
    for (let i = 0; i < outLen; i++) {
      const pos = i * ratio;
      const j = Math.floor(pos);
      const frac = pos - j;
      out[i] = samples[j] * (1 - frac) + (samples[j + 1] ?? samples[j]) * frac;
    }
    return out;
  }

  /** Encode mono Float32 samples as a 16-bit PCM WAV (little-endian). */
  function encodeWav(samples: Float32Array, sampleRate: number): ArrayBuffer {
    const buffer = new ArrayBuffer(44 + samples.length * 2);
    const view = new DataView(buffer);
    const writeStr = (o: number, s: string) => { for (let i = 0; i < s.length; i++) view.setUint8(o + i, s.charCodeAt(i)); };
    writeStr(0, "RIFF");
    view.setUint32(4, 36 + samples.length * 2, true);
    writeStr(8, "WAVE");
    writeStr(12, "fmt ");
    view.setUint32(16, 16, true);   // PCM chunk size
    view.setUint16(20, 1, true);    // format = PCM
    view.setUint16(22, 1, true);    // mono
    view.setUint32(24, sampleRate, true);
    view.setUint32(28, sampleRate * 2, true); // byte rate
    view.setUint16(32, 2, true);    // block align
    view.setUint16(34, 16, true);   // bits per sample
    writeStr(36, "data");
    view.setUint32(40, samples.length * 2, true);
    let o = 44;
    for (let i = 0; i < samples.length; i++, o += 2) {
      const s = Math.max(-1, Math.min(1, samples[i]));
      view.setInt16(o, s < 0 ? s * 0x8000 : s * 0x7fff, true);
    }
    return buffer;
  }

  function fmtTime(s: number): string {
    const m = Math.floor(s / 60), sec = Math.floor(s % 60);
    return `${m}:${sec.toString().padStart(2, "0")}`;
  }

  // Group consecutive utterances by the same speaker for a cleaner transcript view.
  type GroupItem = { speaker: string | null; start: number; items: { idx: number; u: Utterance }[] };
  const groups = $derived.by(() => {
    if (!transcript) return [] as GroupItem[];
    const out: GroupItem[] = [];
    transcript.utterances.forEach((u, idx) => {
      const last = out[out.length - 1];
      if (last && last.speaker === u.speaker) last.items.push({ idx, u });
      else out.push({ speaker: u.speaker, start: u.start, items: [{ idx, u }] });
    });
    return out;
  });

  // ============================================================================
  // Task-oriented screens, standalone de-identify/summarize, and jobs history
  // ============================================================================
  type Screen = "home" | "transcribe" | "meeting" | "deidentify" | "summarize" | "history";
  let screen = $state<Screen>("home");
  function go(s: Screen) {
    screen = s;
    error = "";
    if (s === "history") refreshJobs();
  }

  /** Clear the current working project and return Home for a fresh start. Everything is auto-saved
   *  in Historik, so nothing is lost — reopen a job there to resume it. */
  function newProject() {
    // Don't wipe a capture/job that's still running; just navigate home.
    if (recording || meetingActive || meetingBusy || busy) {
      go("home");
      return;
    }
    transcript = null;
    analysis = null;
    summaryDraft = "";
    audioPath = null;
    audioName = null;
    speakerLabels = {};
    srcMode = "paste";
    srcText = "";
    srcPath = null;
    srcName = null;
    srcHasTables = false;
    deidentDoc = false;
    editingIdx = null;
    dirty = false;
    rejected = new Set();
    useAi = false;
    qaQuestion = "";
    qaHistory = [];
    meetingSysWav = null;
    meetingMicWav = null;
    liveUtterances = [];
    currentJobId = null;
    currentJobCreatedAt = null;
    view = "transcript";
    go("home");
  }

  // ---- Standalone source for de-identify / summarize (no transcript needed) ----
  // "paste" = use srcText · "file" = a loaded .txt/.md/.docx · "transcript" = the in-app transcript.
  let srcMode = $state<"paste" | "file" | "transcript">("paste");
  let srcText = $state("");
  let srcPath = $state<string | null>(null);
  let srcName = $state<string | null>(null);
  let srcHasTables = $state(false);
  // True when the current `analysis` came from a pasted/loaded document (analyze_document → engine.last)
  // rather than the transcript — selects the right copy/export path.
  let deidentDoc = $state(false);

  async function pickSourceDoc() {
    const sel = await open({
      multiple: false,
      filters: [{ name: "Text / dokument", extensions: ["txt", "md", "text", "docx"] }],
    });
    if (typeof sel !== "string") return;
    try {
      const info = await invoke<{ text: string; hasTables: boolean }>("load_document", { path: sel });
      srcPath = sel;
      srcName = sel.split(/[\\/]/).pop() ?? sel;
      srcText = info.text;
      srcHasTables = info.hasTables;
      srcMode = "file";
    } catch (e) {
      error = String(e);
    }
  }
  function clearSource() {
    srcText = "";
    srcPath = null;
    srcName = null;
    srcHasTables = false;
    if (srcMode === "file") srcMode = "paste";
  }

  // ---- Standalone de-identify (pasted text or a loaded doc; sets engine.last) ----
  async function runDeidentify() {
    if (busy) return;
    if (srcMode === "transcript") {
      deidentDoc = false;
      await runAnonymize();
      if (analysis) await saveCurrentJob("deidentify");
      return;
    }
    const hasInput = srcMode === "file" ? !!srcPath : !!srcText.trim();
    if (!hasInput) {
      error = "Klistra in text eller välj en fil först.";
      return;
    }
    busy = true;
    error = "";
    progressMsg = "Avidentifierar…";
    try {
      analysis = await invoke<AnalyzeResult>("analyze_document", {
        args: {
          text: srcMode === "file" ? null : srcText,
          path: srcMode === "file" ? srcPath : null,
          enabled: ALL_KEYS,
          terms,
          useAi,
        },
      });
      rejected = new Set();
      deidentDoc = true;
      view = "review";
      await saveCurrentJob("deidentify");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      progressMsg = "";
    }
  }
  async function copyAnonDoc() {
    try {
      const text = await invoke<string>("copy_anonymized", { rejected: rejectedIds() });
      await navigator.clipboard.writeText(text);
      showToast("Kopierat till urklipp");
    } catch (e) {
      error = String(e);
    }
  }
  async function exportAnonDoc(ext: "txt" | "docx") {
    const path = await save({
      defaultPath: `${(srcName ?? "text").replace(/\.[^.]+$/, "")}_avidentifierad.${ext}`,
      filters: [{ name: ext.toUpperCase(), extensions: [ext] }],
    });
    if (!path) return;
    try {
      await invoke("export_anonymized", { args: { path, rejected: rejectedIds() } });
      showToast("Filen sparades");
    } catch (e) {
      error = String(e);
    }
  }

  // ---- Standalone summarize ----
  async function doSummarize(text: string) {
    if (busy) return;
    if (!summaryDownloaded) {
      error = "Hämta den valda sammanfattningsmodellen först.";
      return;
    }
    if (!text.trim()) {
      error = "Ingen text att sammanfatta.";
      return;
    }
    busy = true;
    error = "";
    progressMsg = "Förbereder…";
    try {
      summaryDraft = await invoke<string>("summarize", {
        args: { text, model: selectedSummaryModel, template: selectedTemplate, customHeadings },
      });
      view = "summary";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      progressMsg = "";
    }
  }
  async function runSummarizeSource() {
    const text = srcMode === "transcript" ? await summaryInputText() : srcText;
    await doSummarize(text);
    if (summaryDraft) await saveCurrentJob("summarize");
  }

  // ---- Jobs / history (auto-saved past work) ----
  type JobMeta = { id: string; title: string; jobType: string; createdAt: string; updatedAt: string };
  let allJobs = $state<JobMeta[]>([]);
  let recentJobs = $state<JobMeta[]>([]);
  let currentJobId = $state<string | null>(null);
  let currentJobCreatedAt = $state<string | null>(null);

  async function refreshJobs() {
    try {
      allJobs = await invoke<JobMeta[]>("list_jobs");
      recentJobs = allJobs.slice(0, 6);
    } catch {
      /* history is best-effort */
    }
  }

  const JOB_LABELS: Record<string, string> = {
    transcribe: "Transkribering",
    meeting: "Möte",
    deidentify: "Avidentifiering",
    summarize: "Sammanfattning",
  };

  function deriveTitle(type: string): string {
    if ((type === "transcribe" || type === "meeting") && audioName) return audioName;
    if (srcName) return srcName;
    const base =
      type === "transcribe" || type === "meeting"
        ? transcript?.utterances?.[0]?.text ?? ""
        : srcMode === "transcript"
          ? transcript?.utterances?.[0]?.text ?? ""
          : srcText;
    const words = (base || "").trim().split(/\s+/).slice(0, 7).join(" ");
    return words || JOB_LABELS[type] || "Jobb";
  }

  async function saveCurrentJob(type: "transcribe" | "deidentify" | "summarize" | "meeting") {
    const now = new Date().toISOString();
    if (!currentJobId) {
      currentJobId = crypto.randomUUID ? crypto.randomUUID() : String(Date.now());
      currentJobCreatedAt = now;
    }
    const job = {
      version: 1,
      id: currentJobId,
      jobType: type,
      title: deriveTitle(type),
      createdAt: currentJobCreatedAt ?? now,
      updatedAt: now,
      transcript: type === "transcribe" || type === "meeting" ? transcript : null,
      speakerLabels,
      audioPath: type === "transcribe" || type === "meeting" ? audioPath : null,
      sourceText: type !== "transcribe" && srcMode !== "transcript" && srcMode !== "file" ? srcText : null,
      sourcePath: type !== "transcribe" && srcMode === "file" ? srcPath : null,
      enabled: ALL_KEYS.filter((k) => enabled[k]),
      terms,
      useAi,
      rejected: rejectedIds(),
      summaryDraft: summaryDraft || null,
      summaryTemplate: selectedTemplate,
      summaryModel: selectedSummaryModel,
      customHeadings,
    };
    try {
      await invoke("save_job", { job });
      await refreshJobs();
    } catch {
      /* non-fatal: failing to persist history shouldn't break the task */
    }
  }

  async function openJobById(id: string) {
    try {
      const j = await invoke<any>("open_job", { id });
      // Reset working state, then hydrate from the job.
      transcript = null;
      analysis = null;
      summaryDraft = "";
      deidentDoc = false;
      currentJobId = j.id;
      currentJobCreatedAt = j.createdAt ?? null;
      speakerLabels = j.speakerLabels ?? {};
      if (Array.isArray(j.terms)) terms = j.terms;
      if (typeof j.useAi === "boolean") useAi = j.useAi;
      if (j.summaryTemplate) selectedTemplate = j.summaryTemplate;
      if (j.summaryModel) selectedSummaryModel = j.summaryModel;
      if (j.customHeadings) customHeadings = j.customHeadings;
      srcText = j.sourceText ?? "";
      srcPath = j.sourcePath ?? null;
      srcName = j.sourcePath ? j.sourcePath.split(/[\\/]/).pop() : null;

      if (j.jobType === "transcribe" || j.jobType === "meeting") {
        transcript = j.transcript ?? null;
        audioPath = j.audioPath ?? null;
        audioName = audioPath ? audioPath.split(/[\\/]/).pop() ?? audioPath : null;
        meetingSysWav = j.jobType === "meeting" ? j.audioPath ?? null : null;
        summaryDraft = j.summaryDraft ?? "";
        view = j.summaryDraft ? "summary" : "transcript";
        screen = "transcribe";
      } else if (j.jobType === "deidentify") {
        srcMode = j.sourcePath ? "file" : "paste";
        screen = "deidentify";
        await runDeidentify(); // re-run to repopulate engine.last + spans (offsets aren't serialized)
        if (Array.isArray(j.rejected) && analysis) rejected = new Set(j.rejected);
      } else {
        transcript = j.transcript ?? null;
        srcMode = j.sourcePath ? "file" : j.sourceText ? "paste" : "transcript";
        summaryDraft = j.summaryDraft ?? "";
        view = "summary";
        screen = "summarize";
      }
      error = "";
      showToast("Jobb öppnat");
    } catch (e) {
      error = String(e);
    }
  }

  async function deleteJobById(id: string) {
    if (!(await ask("Ta bort det här jobbet ur historiken?", { title: "Ta bort jobb", kind: "warning" }))) return;
    try {
      await invoke("delete_job", { id });
      if (currentJobId === id) currentJobId = null;
      await refreshJobs();
    } catch (e) {
      error = String(e);
    }
  }

  function fmtJobDate(iso: string): string {
    const d = new Date(iso);
    if (isNaN(d.getTime())) return iso;
    return d.toLocaleString("sv-SE", { dateStyle: "medium", timeStyle: "short" });
  }
</script>

<div class="app">
  <header>
    <button class="brandbtn" onclick={newProject} title="Hem — börja nytt">
      <svg class="logo" viewBox="0 0 48 48" fill="none" aria-hidden="true">
        <rect x="9" y="10" width="20" height="28" rx="2" fill="#fff" stroke="#111214" stroke-width="2" />
        <rect x="13" y="17" width="12" height="2.6" fill="#111214" />
        <rect x="13" y="22" width="12" height="2.6" fill="#2440ff" />
        <rect x="13" y="27" width="7" height="2.6" fill="#c9ccd2" />
        <path d="M34 16v16M38 20v8M30 21v6" stroke="#2440ff" stroke-width="2.4" stroke-linecap="round" />
      </svg>
      <span class="brand"><h1>Avskrift</h1></span>
    </button>
    <nav class="topnav">
      <button class:on={screen === "transcribe"} onclick={() => go("transcribe")}>Transkribera</button>
      <button class:on={screen === "meeting"} onclick={() => go("meeting")}>Möte</button>
      <button class:on={screen === "deidentify"} onclick={() => { srcMode = transcript ? "transcript" : "paste"; go("deidentify"); }}>Avidentifiera</button>
      <button class:on={screen === "summarize"} onclick={() => { srcMode = transcript ? "transcript" : "paste"; go("summarize"); }}>Sammanfatta</button>
      <button class:on={screen === "history"} onclick={() => go("history")}>Historik</button>
    </nav>
    <div class="spacer"></div>
    <div class="lockbadge"><span class="dot"></span> Allt körs lokalt</div>
  </header>

  {#snippet sourcePicker()}
    <section>
      <h2>Källa</h2>
      <div class="src-modes">
        <label class="radio"><input type="radio" name="src" value="paste" bind:group={srcMode} /> Klistra in text</label>
        <label class="radio"><input type="radio" name="src" value="file" bind:group={srcMode} /> Dokument (.txt/.docx)</label>
        {#if transcript}<label class="radio"><input type="radio" name="src" value="transcript" bind:group={srcMode} /> Från transkriptet</label>{/if}
      </div>
      {#if srcMode === "paste"}
        <textarea class="src-text" bind:value={srcText} rows="8" placeholder="Klistra in texten här…"></textarea>
      {:else if srcMode === "file"}
        {#if srcName}
          <div class="file-chip"><span title={srcPath}>{srcName}</span><button class="link" onclick={clearSource}>rensa</button></div>
        {:else}
          <button class="btn block" onclick={pickSourceDoc}>Välj dokument…</button>
          <p class="hint">.txt, .md eller .docx</p>
        {/if}
      {:else}
        <p class="hint">Använder transkriptet som redan finns i appen.</p>
      {/if}
    </section>
  {/snippet}

  {#if screen === "home"}
    <div class="home">
      <h2 class="big-title">Vad vill du göra?</h2>
      <div class="cards">
        <button class="card" onclick={() => go("transcribe")}>
          <span class="card-ic-wrap">
            <svg class="card-ic" viewBox="0 0 24 24" fill="none">
              <path d="M4 9v6M7 6.5v11M10 9.5v5" stroke="#2440ff" stroke-width="1.7" stroke-linecap="round"/>
              <path d="M14 8.5h6M14 12h6M14 15.5h4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
            </svg>
          </span>
          <h3>Transkribera ljud</h3>
          <p>Ljudfil eller inspelning blir text — med talare och tidsstämplar.</p>
        </button>
        <button class="card" onclick={() => go("meeting")}>
          <span class="card-ic-wrap">
            <svg class="card-ic" viewBox="0 0 24 24" fill="none">
              <rect x="9" y="2.5" width="6" height="11" rx="3" stroke="currentColor" stroke-width="1.7"/>
              <path d="M5.5 11a6.5 6.5 0 0 0 13 0" stroke="#2440ff" stroke-width="1.7" stroke-linecap="round"/>
              <path d="M12 17.5V21" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>
              <circle cx="19" cy="5" r="2.4" fill="#2440ff"/>
            </svg>
          </span>
          <h3>Spela in möte</h3>
          <p>Transkribera ett digitalt möte — din röst och mötesljudet hålls isär som ”Jag” och ”Mötet”.</p>
        </button>
        <button class="card" onclick={() => { srcMode = transcript ? "transcript" : "paste"; go("deidentify"); }}>
          <span class="card-ic-wrap">
            <svg class="card-ic" viewBox="0 0 24 24" fill="none">
              <path d="M12 2.5l7.5 3v4.6c0 5-3.2 7.4-7.5 8.6-4.3-1.2-7.5-3.6-7.5-8.6V5.5L12 2.5z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>
              <path d="M8.5 10.4h5M8.5 13.4h3.4" stroke="#2440ff" stroke-width="1.9" stroke-linecap="round"/>
            </svg>
          </span>
          <h3>Avidentifiera text</h3>
          <p>Maska namn och känsliga uppgifter i en inklistrad text eller ett dokument.</p>
        </button>
        <button class="card" onclick={() => { srcMode = transcript ? "transcript" : "paste"; go("summarize"); }}>
          <span class="card-ic-wrap">
            <svg class="card-ic" viewBox="0 0 24 24" fill="none">
              <rect x="4.5" y="3" width="15" height="18" rx="2.2" stroke="currentColor" stroke-width="1.7"/>
              <circle cx="8.4" cy="8" r="1.1" fill="#2440ff"/><path d="M11 8h5.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
              <circle cx="8.4" cy="12" r="1.1" fill="#2440ff"/><path d="M11 12h5.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
              <circle cx="8.4" cy="16" r="1.1" fill="#2440ff"/><path d="M11 16h3" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
            </svg>
          </span>
          <h3>Sammanfatta text</h3>
          <p>Skapa ett mötesprotokoll eller en kort sammanfattning ur en text.</p>
        </button>
        <button class="card" onclick={() => go("history")}>
          <span class="card-ic-wrap">
            <svg class="card-ic" viewBox="0 0 24 24" fill="none">
              <circle cx="12" cy="12" r="8.5" stroke="currentColor" stroke-width="1.7"/>
              <path d="M12 7v5l3.5 2" stroke="#2440ff" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
          </span>
          <h3>Tidigare jobb</h3>
          <p>{allJobs.length} sparade {allJobs.length === 1 ? "jobb" : "jobb"} — öppna och fortsätt där du slutade.</p>
        </button>
      </div>

      {#if recentJobs.length}
        <div class="recent">
          <h2>Senaste</h2>
          <ul class="job-strip">
            {#each recentJobs as j (j.id)}
              <li>
                <button class="job-row" onclick={() => openJobById(j.id)}>
                  <span class="job-badge {j.jobType}">{JOB_LABELS[j.jobType] ?? j.jobType}</span>
                  <span class="job-title">{j.title}</span>
                  <span class="job-date">{fmtJobDate(j.updatedAt)}</span>
                </button>
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      <button class="link home-open" onclick={openProject}>Öppna sparat projekt (.avskrift)…</button>
    </div>

  {:else if screen === "history"}
    <div class="home">
      <h2 class="big-title">Tidigare jobb</h2>
      {#if !allJobs.length}
        <p class="hint big-hint">Inga sparade jobb än. Allt du transkriberar, avidentifierar eller sammanfattar sparas automatiskt och dyker upp här.</p>
      {:else}
        <ul class="job-list">
          {#each allJobs as j (j.id)}
            <li class="job-item">
              <button class="job-row" onclick={() => openJobById(j.id)}>
                <span class="job-badge {j.jobType}">{JOB_LABELS[j.jobType] ?? j.jobType}</span>
                <span class="job-title">{j.title}</span>
                <span class="job-date">{fmtJobDate(j.updatedAt)}</span>
              </button>
              <button class="x" onclick={() => deleteJobById(j.id)} aria-label="Ta bort">×</button>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

  {:else if screen === "deidentify"}
    <div class="layout">
      <aside class="sidebar">
        {@render sourcePicker()}
        <section class="anon-block">
          <h2>Avidentifiering</h2>
          <select class="profile" value={selectedProfile} onchange={(e) => applyProfile(e.currentTarget.value)}>
            {#each PROFILES as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
          </select>
          <label class="ai-toggle">
            <input type="checkbox" bind:checked={useAi} />
            <span>Djupare granskning (AI)<em>långsammare, fångar fler ledtrådar</em></span>
          </label>
          <button class="btn primary block" onclick={runDeidentify} disabled={busy}>
            {analysis ? "Kör om avidentifiering" : "Avidentifiera"}
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
      </aside>

      <main class="review">
        {#if error}<div class="banner error">{error}</div>{/if}
        {#if srcHasTables}<div class="banner warn">Dokumentet innehåller tabeller — text i tabeller hanteras inte i denna version och tas inte med.</div>{/if}
        {#if busy}
          <div class="state"><div class="spinner"></div><p class="state-title">{progressMsg || "Arbetar…"}</p><p class="state-sub">Allt körs lokalt.</p></div>
        {:else if analysis}
          <div class="review-head">
            <div class="tabs"><span class="tab on">Avidentifiering</span></div>
            <div class="actions">
              <button class="btn primary" onclick={() => (deidentDoc ? copyAnonDoc() : copyAnon())}>Kopiera</button>
              {#if deidentDoc}
                <button class="btn" onclick={() => exportAnonDoc("txt")}>.txt</button>
                <button class="btn" onclick={() => exportAnonDoc("docx")}>Word</button>
              {:else}
                <button class="btn" onclick={() => exportAs("txt", true)}>.txt</button>
                <button class="btn" onclick={() => exportAs("docx", true)}>Word</button>
              {/if}
            </div>
          </div>
          <div class="meta"><strong>{activeCount}</strong> av {analysis.spans.length} träffar avidentifieras</div>
          <div class="document">{#each analysis.segments as seg}{#if seg.span === null}{#if seg.word}<button class="maskword" onclick={() => openMask(seg)} title="Maskera manuellt">{seg.text}</button>{:else}{seg.text}{/if}{:else}{@const info = analysis.spans[seg.span]}{@const active = isActive(seg.span)}{@const off = !enabled[info.category]}<button
                  class="hit" class:active class:rejected={!active && !off} class:disabled={off} class:manual={info.source === "manual"}
                  style="--c:{colorOf(info.category)}"
                  title={active ? `${info.text} → ${info.replacement}` : info.text}
                  onclick={() => toggleSpan(seg.span!)}>{seg.text}</button>{/if}{/each}</div>
          <div class="reassure">
            <svg viewBox="0 0 24 24" fill="none"><path d="M12 3l8 4v5c0 5-3.4 7.7-8 9-4.6-1.3-8-4-8-9V7l8-4z" stroke="#111214" stroke-width="2"/><path d="M9 12l2 2 4-4" stroke="#2440ff" stroke-width="2"/></svg>
            Granska alltid träffarna innan du delar. Ingen automatik fångar 100 %.
          </div>
        {:else}
          <div class="state">
            <svg class="state-icon" viewBox="0 0 24 24" fill="none">
              <path d="M12 2.5l7.5 3v4.6c0 5-3.2 7.4-7.5 8.6-4.3-1.2-7.5-3.6-7.5-8.6V5.5L12 2.5z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>
              <path d="M8.5 10.4h5M8.5 13.4h3.4" stroke="#2440ff" stroke-width="1.7" stroke-linecap="round"/>
            </svg>
            <p class="state-title">Avidentifiera en text</p>
            <p class="state-sub">Klistra in en text eller välj ett dokument till vänster och klicka <strong>Avidentifiera</strong>. Granska träffarna och exportera maskerad text.</p>
          </div>
        {/if}
      </main>
    </div>

  {:else if screen === "summarize"}
    <div class="layout">
      <aside class="sidebar">
        {@render sourcePicker()}
        <section class="anon-block">
          <h2>Sammanfattning</h2>
          <select class="profile" bind:value={selectedTemplate}>
            {#each summaryTemplates as t (t.id)}<option value={t.id}>{t.label}</option>{/each}
            <option value="custom">Egen mall / dagordning…</option>
          </select>
          {#if selectedTemplate === "custom"}
            <textarea class="mt" bind:value={customHeadings} rows="4" placeholder="En rubrik per rad, t.ex.&#10;## Närvarande&#10;## Beslut"></textarea>
          {/if}
          <select class="profile mt" bind:value={selectedSummaryModel}>
            {#each summaryModels as m (m.id)}<option value={m.id}>{m.label}{m.downloaded ? "" : " — hämtas"}</option>{/each}
          </select>
          {#if !summaryDownloaded}
            {#if downloading === selectedSummaryModel}
              <div class="dl"><div class="dl-bar" style="width:{downloadPct}%"></div></div>
              <p class="hint">Hämtar… {downloadPct}%</p>
            {:else}
              <button class="btn block mt" onclick={() => downloadSummaryModel(selectedSummaryModel)} disabled={!!downloading}>
                Hämta modell ({summaryModels.find((m) => m.id === selectedSummaryModel)?.sizeMb ?? "?"} MB)
              </button>
            {/if}
          {/if}
          <button class="btn primary block mt" onclick={runSummarizeSource} disabled={busy || !summaryDownloaded}>
            {summaryDraft ? "Generera om" : "Skapa sammanfattning"}
          </button>
        </section>
      </aside>

      <main class="review">
        {#if error}<div class="banner error">{error}</div>{/if}
        {#if srcHasTables}<div class="banner warn">Dokumentet innehåller tabeller — text i tabeller tas inte med.</div>{/if}
        {#if busy}
          <div class="state"><div class="spinner"></div><p class="state-title">{progressMsg || "Arbetar…"}</p><p class="state-sub">Lokal sammanfattning kan ta en stund.</p></div>
        {:else if summaryDraft}
          <div class="review-head">
            <div class="tabs"><span class="tab on">Sammanfattning</span></div>
            <div class="actions">
              <button class="btn primary" onclick={copySummary}>Kopiera</button>
              <button class="btn" onclick={() => saveSummary("txt")}>.txt</button>
              <button class="btn" onclick={() => saveSummary("docx")}>Word</button>
            </div>
          </div>
          <div class="banner warn">AI-genererat utkast — kan innehålla fel eller utelämnanden. Granska och redigera innan du delar.</div>
          <textarea class="summary-edit" bind:value={summaryDraft} spellcheck="true"></textarea>
        {:else}
          <div class="state">
            <svg class="state-icon" viewBox="0 0 24 24" fill="none">
              <rect x="4.5" y="3" width="15" height="18" rx="2.2" stroke="currentColor" stroke-width="1.5"/>
              <circle cx="8.4" cy="8" r="1.1" fill="#2440ff"/><path d="M11 8h5.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
              <circle cx="8.4" cy="12" r="1.1" fill="#2440ff"/><path d="M11 12h5.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
              <circle cx="8.4" cy="16" r="1.1" fill="#2440ff"/><path d="M11 16h3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
            </svg>
            <p class="state-title">Sammanfatta en text</p>
            <p class="state-sub">Klistra in en text eller välj ett dokument, välj mall och klicka <strong>Skapa sammanfattning</strong>.</p>
          </div>
        {/if}
      </main>
    </div>

  {:else if screen === "meeting"}
    <div class="home">
      <h2 class="big-title">Spela in möte</h2>
      <div class="meeting-card">
        {#if !meetingActive && !meetingBusy}
          <p class="hint big-hint">
            Fångar <strong>din mikrofon</strong> och <strong>mötesljudet</strong> (det som hörs i datorn)
            som två separata spår — så hålls <em>Jag</em> och <em>Mötet</em> isär utan diarisering. Allt körs lokalt.
          </p>
          <div class="consent">
            <svg viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="1.7"/><path d="M12 7.8v5.4" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"/><circle cx="12" cy="16.4" r="1.1" fill="currentColor"/></svg>
            <span>Berätta för deltagarna att du spelar in. Du ansvarar för att inspelningen sker lagligt och med samtycke.</span>
          </div>
          <div class="m-fields">
            <label class="m-field"><span>Modell</span>
              <select class="profile" bind:value={selectedModel}>
                {#each models as m (m.id)}<option value={m.id}>{m.label}{m.downloaded ? "" : " — hämtas"}</option>{/each}
              </select>
            </label>
            <label class="m-field"><span>Språk</span>
              <select class="profile" bind:value={language}>
                {#each LANGUAGES as l (l.code)}<option value={l.code}>{l.label}</option>{/each}
              </select>
            </label>
          </div>
          <label class="ai-toggle">
            <input type="checkbox" bind:checked={meetingLive} />
            <span>Live-text under mötet<em>visar texten medan mötet pågår (kräver hyfsad dator/GPU). Av = transkribera först vid stopp, snällare mot svaga datorer.</em></span>
          </label>
          {#if !selectedDownloaded}
            {#if downloading === selectedModel}
              <div class="dl"><div class="dl-bar" style="width:{downloadPct}%"></div></div>
              <p class="hint">Hämtar modell… {downloadPct}%</p>
            {:else}
              <button class="btn block" onclick={() => downloadModel(selectedModel)} disabled={!!downloading}>
                Hämta modell ({models.find((m) => m.id === selectedModel)?.sizeMb ?? "?"} MB)
              </button>
            {/if}
          {/if}
          <button class="btn primary block big mt" onclick={startMeeting} disabled={!selectedDownloaded}>Starta inspelning</button>
          <p class="hint">Starta mötet (Teams/Zoom/webbläsare) först, så att mötesljudet spelas upp.</p>
        {:else if meetingActive}
          <div class="big-rec"><span class="recdot"></span> Spelar in · {fmtTime(meetingElapsed)}</div>
          <button class="btn primary block big mt" onclick={stopMeeting}>Stoppa &amp; transkribera</button>
          {#if meetingLagging}
            <div class="banner warn">Transkriberingen släpar efter på den här datorn. All text kommer ikapp när du stoppar — men välj gärna en mindre modell, eller stäng av ”Live-text” nästa gång.</div>
          {/if}
          {#if meetingLive}
            {#if liveUtterances.length}
              <div class="live-feed" bind:this={liveFeedEl}>
                {#each liveUtterances as u}
                  <p class="live-line"><span class="live-who {u.source === 'Jag' ? 'me' : 'them'}">{u.source}</span> {u.text}</p>
                {/each}
              </div>
            {:else}
              <p class="hint" style="text-align:center">Lyssnar… text dyker upp inom ~10 s när någon talar. Mötesljudet fångas bara medan något faktiskt spelas upp i datorn.</p>
            {/if}
          {:else}
            <p class="hint" style="text-align:center">Spelar in din röst + mötesljudet. Allt transkriberas när du stoppar.</p>
          {/if}
        {:else}
          <div class="state">
            <div class="spinner"></div>
            <p class="state-title">{progressMsg || "Transkriberar mötet…"}</p>
            <p class="state-sub">Båda spåren transkriberas och slås ihop till ett transkript. Allt körs lokalt.</p>
            {#if transcribePct !== null}<p class="hint">{transcribePct}%</p>{/if}
          </div>
        {/if}
      </div>
    </div>

  {:else}
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
        <label class="ai-toggle">
          <input type="checkbox" bind:checked={translate} />
          <span>Översätt till engelska<em>transkriberar och översätter i samma steg</em></span>
        </label>
      </section>

      <button class="btn primary block big" onclick={runTranscribe}
        disabled={busy || !audioPath || !selectedDownloaded}>
        {busy ? "Arbetar…" : "Transkribera"}
      </button>
      <div class="row">
        <button class="btn grow" onclick={openProject} disabled={busy}>Öppna projekt…</button>
        <button class="btn grow" onclick={saveProject} disabled={busy || !transcript}>
          Spara projekt{dirty ? " •" : ""}
        </button>
      </div>

      {#if transcript}
        {#if meetingSysWav}
          <section class="anon-block">
            <h2>Mötesröster</h2>
            <button class="btn block" onclick={separateMeetingVoices} disabled={busy}>Separera mötesröster</button>
            <p class="hint">Delar upp ”Mötet” i Talare 1, 2 … (din röst förblir ”Jag”).</p>
          </section>
        {/if}
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

        <section class="anon-block">
          <h2>Sammanfattning</h2>
          <select class="profile" bind:value={selectedTemplate}>
            {#each summaryTemplates as t (t.id)}<option value={t.id}>{t.label}</option>{/each}
            <option value="custom">Egen mall / dagordning…</option>
          </select>
          {#if selectedTemplate === "custom"}
            <textarea class="mt" bind:value={customHeadings} rows="4" placeholder="En rubrik per rad, t.ex.&#10;## Närvarande&#10;## Dagordning&#10;## Beslut"></textarea>
          {/if}
          <select class="profile mt" bind:value={selectedSummaryModel}>
            {#each summaryModels as m (m.id)}
              <option value={m.id}>{m.label}{m.downloaded ? "" : " — hämtas"}</option>
            {/each}
          </select>
          {#if !summaryDownloaded}
            {#if downloading === selectedSummaryModel}
              <div class="dl"><div class="dl-bar" style="width:{downloadPct}%"></div></div>
              <p class="hint">Hämtar… {downloadPct}%</p>
            {:else}
              <button class="btn block mt" onclick={() => downloadSummaryModel(selectedSummaryModel)} disabled={!!downloading}>
                Hämta modell ({summaryModels.find((m) => m.id === selectedSummaryModel)?.sizeMb ?? "?"} MB)
              </button>
            {/if}
          {/if}
          {#if analysis}
            <label class="ai-toggle">
              <input type="checkbox" bind:checked={summaryFromAnon} />
              <span>Sammanfatta avidentifierad text<em>använder maskerade namn/uppgifter</em></span>
            </label>
          {/if}
          <button class="btn block mt" onclick={runSummarize} disabled={busy || !summaryDownloaded}>
            {summaryDraft ? "Generera om" : "Skapa sammanfattning"}
          </button>
        </section>

        <section class="anon-block">
          <h2>Rätta återkommande fel</h2>
          <textarea bind:value={correctionInput} rows="3" placeholder="Ett per rad: fel=>rätt&#10;t.ex. kjol=>Tjörn"></textarea>
          <button class="btn block mt" onclick={applyCorrections} disabled={busy || correctionInput.trim() === ""}>
            Tillämpa på hela transkriptet
          </button>
        </section>

        <section class="anon-block">
          <h2>Exportalternativ</h2>
          <label class="ai-toggle">
            <input type="checkbox" bind:checked={exportTimestamps} />
            <span>Tidsstämplar i text/Word</span>
          </label>
          <label class="ai-toggle">
            <input type="checkbox" bind:checked={includeTranscript} />
            <span>Bifoga transkript i sammanfattning<em>protokoll + transkript i ett dokument</em></span>
          </label>
        </section>
      {/if}
    </aside>

    <main class="review">
      {#if error}<div class="banner error">{error}</div>{/if}
      {#if analysis?.warnings.length}{#each analysis.warnings as w}<div class="banner warn">{w}</div>{/each}{/if}

      {#if busy}
        <div class="state">
          <div class="spinner"></div>
          <p class="state-title">{progressMsg || "Arbetar…"}</p>
          {#if transcribePct !== null}
            <div class="progress"><div class="progress-bar" style="width:{transcribePct}%"></div></div>
            <p class="state-sub">{transcribePct}% — transkriberar</p>
          {:else}
            <p class="state-sub">Allt körs lokalt på din dator. Första körningen laddar modellen.</p>
          {/if}
        </div>
      {:else if !transcript}
        <div class="state">
          <svg class="state-icon" viewBox="0 0 24 24" fill="none">
            <path d="M4 9v6M7 6.5v11M10 9.5v5" stroke="#2440ff" stroke-width="1.5" stroke-linecap="round"/>
            <path d="M14 8.5h6M14 12h6M14 15.5h4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <p class="state-title">Välj en ljudfil och transkribera</p>
          <p class="state-sub">Välj modell och språk, slå på <strong>diarisering</strong> för att skilja talare åt, och klicka <strong>Transkribera</strong>. Sedan kan du avidentifiera och exportera.</p>
        </div>
      {:else}
        <div class="review-head">
          <div class="tabs">
            <button class="tab" class:on={view === "transcript"} onclick={() => (view = "transcript")}>Transkript</button>
            <button class="tab" class:on={view === "review"} onclick={() => (view = "review")} disabled={!analysis}>Avidentifiering</button>
            <button class="tab" class:on={view === "summary"} onclick={() => (view = "summary")} disabled={!summaryDraft}>Sammanfattning</button>
            <button class="tab" class:on={view === "qa"} onclick={() => (view = "qa")}>Fråga</button>
          </div>
          <div class="actions">
            {#if view === "summary" && summaryDraft}
              <button class="btn primary" onclick={copySummary}>Kopiera</button>
              <button class="btn" onclick={() => saveSummary("txt")}>.txt</button>
              <button class="btn" onclick={() => saveSummary("docx")}>Word</button>
            {:else if view === "review" && analysis}
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

        {#if audioSrc}
          <div class="player">
            <button class="play" onclick={togglePlay} aria-label={playing ? "Pausa" : "Spela"}>
              {#if playing}
                <svg viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="5" width="4" height="14"/><rect x="14" y="5" width="4" height="14"/></svg>
              {:else}
                <svg viewBox="0 0 24 24" fill="currentColor"><path d="M7 5l12 7-12 7z"/></svg>
              {/if}
            </button>
            <span class="pt">{fmtTime(currentTime)}{audioEl?.duration ? " / " + fmtTime(audioEl.duration) : ""}</span>
            <input
              class="seek"
              type="range"
              min="0"
              max={audioEl?.duration || 0}
              step="0.01"
              value={currentTime}
              oninput={(e) => seekTo(+e.currentTarget.value)}
            />
            <audio
              bind:this={audioEl}
              src={audioSrc}
              preload="auto"
              ontimeupdate={() => (currentTime = audioEl?.currentTime ?? 0)}
              onplay={() => (playing = true)}
              onpause={() => (playing = false)}
              onended={() => (playing = false)}
            ></audio>
          </div>
        {/if}

        {#if view === "transcript"}
          <div class="meta">
            {transcript.utterances.length} segment · modell {transcript.model}{transcript.diarized ? " · diariserad" : ""} · klicka tid för att spela · dubbelklicka text för att rätta
          </div>
          <div class="transcript">
            {#each groups as g}
              <div class="turn">
                {#if g.speaker}
                  <input class="speaker" value={speakerLabels[g.speaker]} oninput={(e) => renameSpeaker(g.speaker!, e.currentTarget.value)} />
                {/if}
                {#each g.items as it}
                  <p class="utext">
                    <span class="ts" role="button" tabindex="0"
                      onclick={() => seekTo(it.u.start)}
                      onkeydown={(e) => e.key === "Enter" && seekTo(it.u.start)}
                    >{fmtTime(it.u.start)}</span>{#if editingIdx === it.idx}<textarea
                        class="edit"
                        bind:value={editText}
                        onblur={commitEdit}
                        onkeydown={(e) => { if (e.key === "Escape") cancelEdit(); if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) commitEdit(); }}
                      ></textarea>{:else}<span
                        class="body"
                        role="button"
                        tabindex="0"
                        ondblclick={() => startEdit(it.idx)}
                        onkeydown={(e) => e.key === "Enter" && startEdit(it.idx)}
                      >{#if it.u.words && it.u.words.length}{#each it.u.words as w}<button
                              class="word"
                              class:playing={playing && currentTime >= w.start && currentTime < w.end}
                              onclick={() => seekTo(w.start)}
                            >{w.text}</button>{" "}{/each}{:else}<span
                            class="useg"
                            class:playing={activeUtterance === it.idx}
                          >{it.u.text}</span>{/if}</span>{/if}
                  </p>
                {/each}
              </div>
            {/each}
          </div>
        {:else if view === "qa"}
          <div class="qa">
            {#if qaHistory.length}
              <div class="qa-history">
                {#each qaHistory as item}
                  <div class="qa-item">
                    <p class="qa-q">{item.q}</p>
                    <p class="qa-a">{item.a}</p>
                  </div>
                {/each}
              </div>
            {:else}
              <p class="hint big-hint qa-empty">Ställ en fråga om mötet — t.ex. ”Vilka beslut togs?” eller ”Vad ska jag göra till nästa vecka?”. Svaren bygger enbart på transkriptet.</p>
            {/if}
            <form class="qa-form" onsubmit={(e) => { e.preventDefault(); askMeeting(); }}>
              <input class="qa-input" bind:value={qaQuestion} placeholder="Fråga mötet…" disabled={qaBusy} />
              <button class="btn primary" type="submit" disabled={qaBusy || !qaQuestion.trim() || !summaryDownloaded}>{qaBusy ? "…" : "Fråga"}</button>
            </form>
            {#if !summaryDownloaded}<p class="hint">Q&amp;A använder sammanfattningsmodellen — hämta den i Sammanfattning-panelen först.</p>{/if}
          </div>
        {:else if view === "review" && analysis}
          <div class="meta"><strong>{activeCount}</strong> av {analysis.spans.length} träffar avidentifieras</div>
          <div class="document">{#each analysis.segments as seg}{#if seg.span === null}{#if seg.word}<button class="maskword" onclick={() => openMask(seg)} title="Maskera manuellt">{seg.text}</button>{:else}{seg.text}{/if}{:else}{@const info = analysis.spans[seg.span]}{@const active = isActive(seg.span)}{@const off = !enabled[info.category]}<button
                  class="hit" class:active class:rejected={!active && !off} class:disabled={off} class:manual={info.source === "manual"}
                  style="--c:{colorOf(info.category)}"
                  title={active ? `${info.text} → ${info.replacement}` : info.text}
                  onclick={() => toggleSpan(seg.span!)}>{seg.text}</button>{/if}{/each}</div>
          <div class="reassure">
            <svg viewBox="0 0 24 24" fill="none"><path d="M12 3l8 4v5c0 5-3.4 7.7-8 9-4.6-1.3-8-4-8-9V7l8-4z" stroke="#111214" stroke-width="2"/><path d="M9 12l2 2 4-4" stroke="#2440ff" stroke-width="2"/></svg>
            Granska alltid träffarna innan du delar. Ingen automatik fångar 100 %.
          </div>
        {:else if view === "summary"}
          <div class="banner warn">AI-genererat utkast — kan innehålla fel eller utelämnanden. Granska och redigera innan du delar.</div>
          <textarea class="summary-edit" bind:value={summaryDraft} spellcheck="true"></textarea>
        {/if}
      {/if}
    </main>
  </div>
  {/if}
</div>

{#if toast}
  <div class="toast"><span class="accentbar"></span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6"><path d="M5 13l4 4L19 7"/></svg>{toast}</div>
{/if}

{#if maskTarget}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="modal-backdrop" onclick={() => (maskTarget = null)} role="presentation">
    <div class="modal" role="dialog" aria-modal="true" onclick={(e) => e.stopPropagation()}>
      <h3 class="modal-title">Maskera ”{maskTarget.text}”</h3>
      <label class="modal-field">Typ
        <select bind:value={maskCategory}>
          {#each CATEGORIES as c (c.key)}<option value={c.key}>{c.label}</option>{/each}
        </select>
      </label>
      <label class="modal-field">Ersätt med (valfritt)
        <input
          bind:value={maskCustom}
          placeholder={"Lämna tomt → " + (CATEGORIES.find((c) => c.key === maskCategory)?.label ?? "") + " N"}
          onkeydown={(e) => { if (e.key === "Enter") confirmMask(); if (e.key === "Escape") maskTarget = null; }}
        />
      </label>
      <div class="modal-actions">
        <button class="btn" onclick={() => (maskTarget = null)}>Avbryt</button>
        <button class="btn primary" onclick={confirmMask}>Maskera</button>
      </div>
    </div>
  </div>
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
  select.profile.mt { margin-top: 8px; }
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
  .utext { margin: 3px 0 0; line-height: 1.8; font-size: 15.5px; }
  .ts { font-size: 11px; color: var(--faint); font-variant-numeric: tabular-nums; margin-right: 10px; cursor: pointer; }
  .ts:hover { color: var(--accent); }
  .word, .useg { border: none; background: none; font: inherit; line-height: inherit; color: inherit; cursor: pointer; padding: 0 1px; border-radius: 3px; }
  .word:hover, .useg:hover { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .word.playing, .useg.playing { background: color-mix(in srgb, var(--accent) 22%, transparent); color: var(--ink); }
  .body { cursor: text; border-radius: 3px; }
  .body:hover { background: color-mix(in srgb, var(--ink) 5%, transparent); }
  .edit { width: 100%; box-sizing: border-box; font: inherit; font-size: 15.5px; line-height: 1.7; border: 1px solid var(--accent); border-radius: 4px; padding: 6px 9px; resize: vertical; margin-top: 2px; }

  .progress { width: 220px; height: 7px; background: var(--line); border-radius: 4px; margin: 4px auto 10px; overflow: hidden; }
  .progress-bar { height: 100%; background: var(--accent); transition: width .25s; }

  .row { display: flex; gap: 8px; margin: 8px 0 22px; }
  .grow { flex: 1; }

  .player { display: flex; align-items: center; gap: 12px; padding: 10px 14px; margin-bottom: 14px; background: #f6f7ff; border: 1px solid #dfe3ff; border-radius: 6px; }
  .play { flex: none; width: 36px; height: 36px; border-radius: 50%; border: none; background: var(--accent); color: #fff; cursor: pointer; display: inline-flex; align-items: center; justify-content: center; }
  .play svg { width: 16px; height: 16px; }
  .pt { font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .seek { flex: 1; accent-color: var(--accent); cursor: pointer; }

  .document { flex: 1; overflow: auto; white-space: pre-wrap; line-height: 2.1; font-size: 16px; max-width: 76ch; padding: 4px 2px; }
  .summary-edit { flex: 1; width: 100%; box-sizing: border-box; resize: none; font: inherit; font-size: 15px; line-height: 1.7; color: var(--ink); border: 1px solid var(--line-2); border-radius: 6px; padding: 18px 20px; max-width: 80ch; }
  .summary-edit:focus { outline: none; border-color: var(--accent); }
  .hit { border: none; background: none; font: inherit; line-height: inherit; cursor: pointer; padding: 0 1px 1px; border-bottom: 2px solid var(--c); transition: background .14s; color: inherit; }
  .hit:hover { background: color-mix(in srgb, var(--c) 13%, transparent); }
  .hit.rejected { border-bottom: 2px dotted var(--faint); text-decoration: line-through; color: var(--faint); }
  .hit.disabled { border-bottom: none; cursor: default; }

  .reassure { margin-top: 16px; padding-top: 14px; border-top: 1px solid var(--line); font-size: 12.5px; color: var(--muted); display: flex; align-items: center; gap: 9px; }
  .reassure svg { width: 15px; height: 15px; color: var(--accent); }

  .state { margin: auto; text-align: center; color: var(--muted); max-width: 400px; padding: 30px; }
  .state-icon { width: 44px; height: 44px; color: var(--faint); margin-bottom: 14px; }
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

  /* ---- task shell: brand-as-home button + discreet top nav ---- */
  .brandbtn { display: flex; align-items: flex-end; gap: 12px; background: none; border: none; padding: 0; cursor: pointer; }
  .brandbtn .brand h1 { transition: color .14s; }
  .brandbtn:hover .brand h1 { color: var(--accent); }
  .topnav { display: flex; gap: 2px; margin-bottom: 6px; }
  .topnav button { font: inherit; font-size: 13.5px; font-weight: 500; border: none; background: none; color: var(--faint); padding: 5px 11px; border-radius: 3px; cursor: pointer; transition: .14s; }
  .topnav button:hover { color: var(--ink); }
  .topnav button.on { color: var(--ink); background: color-mix(in srgb, var(--accent) 10%, transparent); }

  /* ---- home / history ---- */
  .home { flex: 1; overflow: auto; padding: 46px 40px 60px; max-width: 920px; width: 100%; margin: 0 auto; box-sizing: border-box; }
  .big-title { font-family: "Instrument Serif", serif; font-weight: 400; font-size: 32px; color: var(--ink); margin: 0 0 26px; letter-spacing: 0; text-transform: none; }
  .cards { display: grid; grid-template-columns: repeat(2, 1fr); gap: 14px; }
  .card { text-align: left; background: var(--bg); border: 1px solid var(--line-2); border-radius: 6px; padding: 22px 22px 20px; cursor: pointer; transition: border-color .14s, box-shadow .14s, transform .14s; font: inherit; color: var(--ink); }
  .card:hover { border-color: var(--accent); box-shadow: 0 10px 30px rgba(36,64,255,.08); transform: translateY(-1px); }
  .card-ic-wrap { width: 46px; height: 46px; border-radius: 12px; background: color-mix(in srgb, var(--accent) 8%, transparent); display: flex; align-items: center; justify-content: center; margin-bottom: 14px; transition: background .14s; }
  .card:hover .card-ic-wrap { background: color-mix(in srgb, var(--accent) 15%, transparent); }
  .card-ic { width: 25px; height: 25px; color: var(--ink); }
  .card h3 { font-size: 16px; font-weight: 600; margin: 0 0 5px; }
  .card p { font-size: 13px; color: var(--muted); margin: 0; line-height: 1.5; }

  .recent { margin-top: 34px; }
  .recent h2 { font-size: 11px; letter-spacing: .15em; text-transform: uppercase; color: var(--faint); margin: 0 0 11px; font-weight: 600; }
  .job-strip, .job-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .job-item { display: flex; align-items: center; gap: 6px; }
  .job-row { flex: 1; display: flex; align-items: center; gap: 12px; text-align: left; background: var(--bg); border: 1px solid var(--line); border-radius: 4px; padding: 11px 13px; cursor: pointer; font: inherit; color: var(--ink); transition: border-color .14s, background .14s; min-width: 0; }
  .job-row:hover { border-color: var(--accent); background: #fafbff; }
  .job-badge { font-size: 10.5px; font-weight: 600; letter-spacing: .04em; text-transform: uppercase; padding: 3px 8px; border-radius: 999px; white-space: nowrap; color: #fff; background: var(--faint); }
  .job-badge.transcribe { background: #2440ff; }
  .job-badge.deidentify { background: #be123c; }
  .job-badge.summarize { background: #0d9488; }
  .job-badge.meeting { background: #7c3aed; }
  .meeting-card { max-width: 540px; margin: 6px auto 0; display: flex; flex-direction: column; gap: 14px; text-align: left; }
  .consent { display: flex; gap: 10px; align-items: flex-start; padding: 12px 14px; border-radius: 12px; background: color-mix(in srgb, #f59e0b 12%, transparent); border: 1px solid color-mix(in srgb, #f59e0b 30%, transparent); font-size: 13px; line-height: 1.45; color: #7c5410; }
  .consent svg { width: 22px; height: 22px; flex-shrink: 0; color: #b45309; }
  .m-fields { display: flex; gap: 12px; }
  .m-field { flex: 1; display: flex; flex-direction: column; gap: 5px; font-size: 12.5px; font-weight: 600; color: #5b6270; }
  .m-field .profile { width: 100%; }
  .big-rec { font-size: 17px; font-weight: 600; display: flex; align-items: center; gap: 10px; justify-content: center; padding: 16px 0 2px; }
  .live-feed { max-height: 340px; overflow-y: auto; text-align: left; background: color-mix(in srgb, var(--accent) 4%, #fff); border: 1px solid color-mix(in srgb, var(--accent) 12%, transparent); border-radius: 12px; padding: 12px 14px; display: flex; flex-direction: column; gap: 7px; margin-top: 4px; }
  .live-line { margin: 0; font-size: 14px; line-height: 1.45; }
  .live-who { display: inline-block; font-size: 10px; font-weight: 700; letter-spacing: .03em; padding: 1px 7px; border-radius: 999px; color: #fff; margin-right: 6px; }
  .live-who.me { background: #2440ff; }
  .live-who.them { background: #7c3aed; }
  .qa { display: flex; flex-direction: column; gap: 14px; max-width: 760px; }
  .qa-history { display: flex; flex-direction: column; gap: 16px; }
  .qa-item { display: flex; flex-direction: column; gap: 6px; }
  .qa-q { margin: 0; font-weight: 600; font-size: 15px; color: var(--ink); }
  .qa-q::before { content: "Du: "; color: var(--accent); }
  .qa-a { margin: 0; font-size: 15px; line-height: 1.55; white-space: pre-wrap; background: #f7f8fa; border: 1px solid var(--line); border-radius: 12px; padding: 12px 14px; }
  .qa-empty { margin: 0; }
  .qa-form { display: flex; gap: 8px; position: sticky; bottom: 0; background: var(--bg); padding: 6px 0; }
  .qa-input { flex: 1; padding: 11px 14px; border: 1px solid var(--line-2); border-radius: 10px; font: inherit; }
  .qa-input:focus { outline: none; border-color: var(--accent); }
  .maskword { display: inline; cursor: pointer; border: none; background: none; font: inherit; color: inherit; padding: 0 1px; border-radius: 3px; }
  .maskword:hover { background: #fff3cd; box-shadow: 0 0 0 1px #f59e0b; }
  .hit.manual { text-decoration: underline; text-decoration-thickness: 2px; text-underline-offset: 2px; }
  .modal-backdrop { position: fixed; inset: 0; background: rgba(17,18,20,.45); display: flex; align-items: center; justify-content: center; z-index: 50; }
  .modal { background: #fff; border-radius: 16px; padding: 22px 24px; width: min(420px, 90vw); box-shadow: 0 20px 60px rgba(0,0,0,.25); display: flex; flex-direction: column; gap: 14px; }
  .modal-title { margin: 0; font-size: 17px; font-weight: 600; }
  .modal-field { display: flex; flex-direction: column; gap: 5px; font-size: 13px; font-weight: 600; color: var(--muted); }
  .modal-field select, .modal-field input { padding: 9px 11px; border: 1px solid var(--line-2); border-radius: 9px; font: inherit; font-weight: 400; color: var(--ink); }
  .modal-field select:focus, .modal-field input:focus { outline: none; border-color: var(--accent); }
  .modal-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
  .job-title { flex: 1; font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .job-date { font-size: 12px; color: var(--faint); white-space: nowrap; }
  .home-open { display: inline-block; margin-top: 30px; }
  .big-hint { font-size: 14px; line-height: 1.6; max-width: 520px; }

  /* ---- standalone source picker ---- */
  .src-modes { display: flex; flex-direction: column; gap: 6px; margin-bottom: 10px; }
  .radio { display: flex; align-items: center; gap: 8px; font-size: 13.5px; cursor: pointer; }
  .src-text { width: 100%; box-sizing: border-box; font: inherit; font-size: 13.5px; line-height: 1.5; border: 1px solid var(--line-2); border-radius: 4px; padding: 9px 11px; resize: vertical; }
  .src-text:focus { outline: none; border-color: var(--accent); }
</style>
