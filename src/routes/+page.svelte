<script lang="ts">
  import {invokeWork,isWorkCancelled} from "$lib/work";
  import "$lib/meeting-workspace.css";
  import WorkControl from "$lib/WorkControl.svelte";
  import { tick, onMount } from "svelte";
  import TranscriptView from '$lib/TranscriptView.svelte';
  import ModelSettings from '$lib/ModelSettings.svelte';
  import Dictation from "$lib/Dictation.svelte";
  import AppNavigation from "$lib/AppNavigation.svelte";
  import VersionsDialog from "$lib/VersionsDialog.svelte";
  import GroundedDraft from '$lib/GroundedDraft.svelte';
  import ReviewComparison from '$lib/ReviewComparison.svelte';
  import ReviewProfiles from '$lib/ReviewProfiles.svelte';
  import type {GroundedWork,SourceUnit} from '$lib/grounded';
  import ExportDialog, { type ExportChoice } from "$lib/ExportDialog.svelte";
  import { createSaveQueue } from "$lib/save-queue";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import type { DictationSnapshot } from "$lib/dictation";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { listen } from "@tauri-apps/api/event";
  import { open, save, ask } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
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
  type Segment = { text: string; span: number | null; start: number; end: number; word: boolean; para: number };
  type AnalyzeResult = {
    snapshot?: { text: string; spans: unknown[]; paraRanges: [number,number][] };
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
    { key: "url", label: "Webbadress", color: "#0369a1" },
    { key: "tid", label: "Tid", color: "#d97706" },
    { key: "handelse", label: "Händelse", color: "#0d9488" },
    { key: "diagnos", label: "Diagnos", color: "#b45309" },
    { key: "medicin", label: "Medicin", color: "#a21caf" },
    { key: "egen", label: "Egen ordlista", color: "#db2777" },
    { key: "ovrigt", label: "Övrigt (AI)", color: "#64748b" },
  ];
  const ALL_KEYS = CATEGORIES.map((c) => c.key);
  const colorOf = (key: string) => CATEGORIES.find((c) => c.key === key)?.color ?? "#888";

  const IDENTITY = ["person", "personnummer", "telefon", "epost", "ip_adress", "url", "plats", "organisation", "egen", "ovrigt"];
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
  let view = $state<"transcript" | "review" | "summary" | "qa" | "notes" | "overview" | "actions">("transcript");
  let modelsOpen = $state(false);
  let transcriptToolsOpen = $state(false);
  let transcriptView = $state<TranscriptView>();
  function openModels() { modelsOpen = true; }
  async function setDictationModel(model:string) {
    if (!dictation || dictation.phase !== 'idle') throw new Error('Stoppa diktatet innan du ändrar modell.');
    await invoke('configure_dictation', {settings:{...dictation.settings,model}});
    updateDictation(await invoke<DictationSnapshot>('dictation_snapshot'));
  }
  let sidebarCollapsed = $state(false); // hide the workspace controls panel to give the content full width

  // ---- Summarisation ----
  type SummaryModel = { id: string; label: string; sizeMb: number; downloaded: boolean };
  type TemplateInfo = { id: string; label: string };
  let summaryModels = $state<SummaryModel[]>([]);
  let summaryTemplates = $state<TemplateInfo[]>([]);
  let selectedSummaryModel = $state("qwen2.5-3b");
  let selectedTemplate = $state("protokoll");
  let customHeadings = $state("## Närvarande\n## Dagordning\n## Beslut\n## Åtgärder");
  let summaryFromAnon = $state(false);

  // ---- Copy for AI (prompt + transcript/de-identified text, for an external chat AI) ----
  const DEFAULT_AI_PROMPTS = [
    { id: "sammanfatta", label: "Sammanfatta", body: "Sammanfatta mötet/texten nedan på svenska, tydligt och strukturerat: kort bakgrund, de viktigaste punkterna, fattade beslut, och åtgärder (vem – vad – när) om det framgår. Var saklig och håll dig strikt till innehållet. Saknas något: skriv \"framgår ej\". Hitta inte på." },
    { id: "atgarder", label: "Åtgärder & beslut", body: "Lista på svenska alla beslut och åtgärder från texten nedan som en punktlista, var och en med: åtgärd, ansvarig och deadline/uppföljning. Ta bara med det som faktiskt sägs. Står ansvarig eller tid inte uttryckligen: skriv \"framgår ej\". Hitta inte på." },
    { id: "analysera", label: "Analysera", body: "Analysera texten nedan på svenska: vilka teman och mönster återkommer, vilka behov och eventuella risker framträder, och vad är oklart eller motsägelsefullt? Skilj tydligt på vad som faktiskt sägs i texten och dina egna tolkningar. Hitta inte på fakta." },
    { id: "foraldrabrev", label: "Föräldrabrev", body: "Skriv ett vänligt, tydligt och respektfullt brev på svenska till en vårdnadshavare utifrån texten nedan: vad som tagits upp, vad som beslutats och nästa steg. Undvik fackjargong och förkortningar. Lägg inte till något som inte framgår av texten." },
    { id: "anteckning", label: "Tjänste-/journalanteckning", body: "Skriv en saklig och formell anteckning på svenska utifrån texten nedan: sammanhang och datum om det framgår, närvarande och roller, vad som behandlades, fattade beslut och planerad uppföljning. Neutral ton, inga värderingar, endast det som framgår." },
    { id: "egen", label: "Egen prompt", body: "" },
  ];
  const AI_PROMPTS_KEY = "avskrift.aiPrompts.v1";
  // Targets for "Öppna i …". `q` = URL that prefills the composer (ChatGPT/Claude); null = no
  // reliable prefill, so we just open the chat and rely on the clipboard + paste.
  const AI_TARGETS = [
    { id: "chatgpt", label: "ChatGPT", url: "https://chatgpt.com/", q: "https://chatgpt.com/?q=" },
    { id: "claude", label: "Claude", url: "https://claude.ai/new", q: "https://claude.ai/new?q=" },
    { id: "gemini", label: "Gemini", url: "https://gemini.google.com/app", q: null },
    { id: "copilot", label: "Copilot", url: "https://copilot.microsoft.com/", q: null },
  ];
  // Encoded-URL length above which we skip prefill (long text won't fit / gets truncated) and
  // just open the chat for a paste. Keeps sensitive-ish text out of very long URLs too.
  const AI_PREFILL_MAX = 6000;
  let aiOpen = $state(false);
  let aiSource = $state<"anon" | "summary" | "transcript">("anon");
  let aiText = $state("");
  let aiDeid = $state(false);
  let aiUseOriginal = $state(false);
  let aiPromptBank = $state<{ id: string; label: string; body: string }[]>([]);
  let aiPromptId = $state("sammanfatta");
  let aiPromptDraft = $state("");
  let aiContext = $state("");
  let summaryDraft = $state("");
  let summaryAnonymized = $state(false);
  const summaryDownloaded = $derived(summaryModels.find((m) => m.id === selectedSummaryModel)?.downloaded ?? false);

  // ---- Editing, corrections, projects, export options ----
  let editingIdx = $state<number | null>(null);
  let editText = $state("");
  let editMode = $state(false); // when on, a single click opens a line for editing instead of seeking
  let undoStack = $state<Transcript[]>([]); // snapshots before each transcript edit (for Ångra)
  let dirty = $state(false); // unsaved edits since last transcribe/open
  let saveState = $state<"idle" | "pending" | "saving" | "saved" | "error">("idle");
  let saveError = $state("");
  let savedAt = $state("");
  let originalTranscript = $state<Transcript | null>(null);
  let originalSourceText = $state<string | null>(null);
  let groundedWork = $state<GroundedWork|null>(null);
  let reviewApprovedBasis = $state<string|null>(null);
  const reviewBasis = $derived.by(()=>JSON.stringify([analysis?.snapshot??analysis?.text,rejectedIds()]));
  const groundedSources = $derived<SourceUnit[]>(transcript?.utterances.map((u,i)=>({id:`u${i}`,text:(u.speaker?`${speakerLabels[u.speaker]??u.speaker}: `:'')+u.text,start:u.start})).filter(s=>s.text.trim())??[]);
  const groundedContext = $derived.by(()=>JSON.stringify([currentJobId,audioPath,meetingMixWav]));
  function saveGrounded(value:GroundedWork){groundedWork=value;saveWorkspace();}
  async function useGrounded(text:string){
    const basis=JSON.stringify([groundedContext,groundedSources]);
    if(!(await checkpointWork()))return;
    if(basis!==JSON.stringify([groundedContext,groundedSources])){error='Underlaget ändrades. Granska det nya underlaget först.';return;}
    summaryDraft=text;summaryAnonymized=false;summaryBasis=JSON.stringify({source:'transcript',text:transcript?.utterances.map(u=>u.text).join('\n')});saveWorkspace();
  }
  async function seekGrounded(source:SourceUnit){if(source.start===null)return;tab('transcript');await tick();const i=transcript?.utterances.findIndex(u=>u.start===source.start)??-1;await transcriptView?.reveal(i);seekTo(source.start);}
  function addGroundedAction(text:string, source?:{quote:string;start:number|null}){actions=[...actions,{id:crypto.randomUUID(),text,source:source?{...source,jobId:currentJobId??undefined}:undefined,done:false,assignee:'',due:''}];saveWorkspace();}
  let summaryBasis = $state<string | null>(null);
  let versionsOpen = $state(false);
  let restoringJob = false;
  let dictationPanel: Dictation;
  let dictationDirty = $state(false);
  const reviewStale = $derived.by(() => !!analysis && analysis.text !== (deidentDoc ? srcText : transcript?.utterances.map(u => u.text).join("\n")));
  const summaryStale = $derived.by(() => {
    if (!summaryDraft || !summaryBasis) return false;
    try { const basis=JSON.parse(summaryBasis);return basis.text !== (basis.source === "transcript" ? transcript?.utterances.map(u=>u.text).join("\n") : srcText); }
    catch { return true; }
  });
  let editRevision = 0;
  const saveQueue = createSaveQueue();
  let exportOpen = $state(false);
  let exportInitial = $state("transcript");
  let exportChoices = $state<ExportChoice[]>([]);
  let correctionInput = $state("");
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
  let recSaving = $state(false); // encoding + saving the take after stop (so a slow save isn't mistaken for a hang)
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
  let meetingMixWav = $state<string | null>(null); // mixed playback track (mic + meeting, echo removed)
  // Live transcription feed (filled by avskrift:meeting-utterance events during a meeting).
  let liveUtterances = $state<{ source: string; start: number; end: number; text: string }[]>([]);
  let liveFeedEl = $state<HTMLDivElement | null>(null);
  let meetingLive = $state(true); // transcribe live during the meeting (vs only after stop)
  let meetingShowLive = $state(true); // live-text pane visible during a meeting
  let meetingShowNotes = $state(false); // notes/actions pane visible during a meeting (the "Anteckningar"-knapp)
  let retranscribeDiarize = $state(true); // after "Transkribera om", auto-split "Mötet" into speakers
  let retranscribeEchoCancel = $state(false); // strip meeting audio that leaked into the mic before transcribing "Jag"
  let meetingLagging = $state(false); // worker fell behind real time (weak hardware)
  // Background work keeps its own stable project identity.
  let bgMeetings = $state<{ id: string; title: string; msg: string }[]>([]);

  // ---- Meeting Q&A ("Fråga mötet") — works on any transcript ----
  let qaQuestion = $state("");
  let qaHistory = $state<{ q: string; a: string }[]>([]);
  let qaBusy = $state(false);

  // ---- Playback (synced with the transcript) ----
  let audioEl = $state<HTMLAudioElement | null>(null);
  let playing = $state(false);
  let currentTime = $state(0);
  // Asset-protocol URL so the webview can stream the local file (see tauri.conf assetProtocol). For
  // meetings, prefer the mixed track (your echo-cleaned mic + the meeting) so you hear yourself
  // without echo; fall back to the system recording when no mix exists.
  let playbackTrack = $state("mix");
  const audioSrc = $derived(
    (playbackTrack==="mic"?meetingMicWav:playbackTrack==="system"?meetingSysWav:meetingMixWav||audioPath) ? convertFileSrc((playbackTrack==="mic"?meetingMicWav:playbackTrack==="system"?meetingSysWav:meetingMixWav||audioPath)!) : "",
  );

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

  // ---- Process state ----
  let busy = $state(false);
  let progressMsg = $state("");
  let downloading = $state<string | null>(null);
  let downloadPct = $state(0);
  let error = $state("");
  let toast = $state("");
  let appVersion = $state(""); // shown in the header so it's clear which version is running

  const selectedDownloaded = $derived(models.find((m) => m.id === selectedModel)?.downloaded ?? false);
  const hasActiveJob = $derived(!!(transcript || analysis || summaryDraft));

  $effect(() => {
    refreshModels();
    refreshSummaryModels();
    refreshJobs();
    loadAllActions();
    invoke<TemplateInfo[]>("list_summary_templates").then((t) => (summaryTemplates = t)).catch(() => {});
    const saved = localStorage.getItem("avskrift_terms");
    if (saved) terms = JSON.parse(saved);
    lastCategory = localStorage.getItem("avskrift.lastCategory") ?? "";
    currentCategory = lastCategory;
    try {
      pendingFolders = JSON.parse(localStorage.getItem("avskrift.pendingFolders") ?? "[]");
    } catch {
      pendingFolders = [];
    }
    // Pick a sensible default Whisper model + meeting mode for this machine (user can override).
    const savedModel = localStorage.getItem("avskrift_model");
    selectedModel = savedModel || hwDefaultModel();
    selectedSummaryModel = localStorage.getItem('avskrift.textModel') || 'qwen2.5-3b';
    meetingLive = !isWeakHardware();
    void getVersion().then((v) => (appVersion = v)).catch(() => {});
  });

  // Remember the chosen Whisper model across sessions.
  $effect(() => { localStorage.setItem("avskrift_model", selectedModel); });

  $effect(() => { localStorage.setItem('avskrift.textModel', selectedSummaryModel); });

  // Remember the last folder so new jobs are filed there automatically.
  $effect(() => { localStorage.setItem("avskrift.lastCategory", lastCategory); });

  // Persist created-but-empty folders so they survive reloads.
  $effect(() => { localStorage.setItem("avskrift.pendingFolders", JSON.stringify(pendingFolders)); });

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
    const mw = listen<string>("avskrift:meeting-warning", e=>meetingWarning=e.payload);
    const ml = listen<boolean>("avskrift:meeting-lag", () => (meetingLagging = true));
    // Background-meeting progress → update the header chip so a long run doesn't look stuck.
    const mp = listen<{ token: string; msg: string }>("avskrift:meeting-progress", (e) => {
      bgMeetings = bgMeetings.map((m) => (m.id === e.payload.token ? { ...m, msg: e.payload.msg } : m));
    });
    // A background meeting finished transcribing → save it straight to Bibliotek.
    const md = listen<any>("avskrift:meeting-done", (e) => { void onMeetingDone(e.payload); });
    const mf = listen<{ token: string; error: string }>("avskrift:meeting-failed", (e) => {

      bgMeetings = bgMeetings.filter((m) => m.id !== e.payload.token);
      // The recoverable project (with its audio) was already saved at stop; leave it in History so the
      // user can re-transcribe it — don't discard it just because the auto-run failed.
      void refreshJobs();
      error = `Mötet kunde inte transkriberas automatiskt – ljudet är sparat, öppna projektet i Bibliotek och välj "Transkribera om". (${e.payload.error})`;
    });
    return () => {
      p.then((f) => f());
      d.then((f) => f());
      pc.then((f) => f());
      mu.then((f) => f());
      mw.then((f)=>f());
      ml.then((f) => f());
      mp.then((f) => f());
      md.then((f) => f());
      mf.then((f) => f());
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
    if (busy || qaBusy || actionsBusy || recording || meetingActive || meetingBusy) return;
    if (!(await flushCurrentSave())) return;
    const selected = await open({
      multiple: false,
      filters: [{ name: "Ljud", extensions: ["mp3", "wav", "m4a", "ogg", "flac", "webm", "mp4", "aac"] }],
    });
    if (typeof selected === "string") {
      await newProject();
      if (currentJobId || saveState === "error") return;
      screen = "transcribe";
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
      if (reviewStale || deidentDoc) throw new Error("Granska det aktuella transkriptet igen innan du använder avidentifierad text.");
      try {
        bodies = await invoke<string[]>("anonymized_segments", { rejected: rejectedIds() });
      } catch (e) {
        throw e;
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

  function loadAiPrompts() {
    let bank: { id: string; label: string; body: string }[] | null = null;
    try {
      const raw = localStorage.getItem(AI_PROMPTS_KEY);
      if (raw) bank = JSON.parse(raw);
    } catch {
      /* ignore corrupt storage */
    }
    aiPromptBank = Array.isArray(bank) && bank.length ? bank : DEFAULT_AI_PROMPTS.map((p) => ({ ...p }));
  }

  function saveAiPrompts() {
    try {
      localStorage.setItem(AI_PROMPTS_KEY, JSON.stringify(aiPromptBank));
    } catch {
      /* storage may be unavailable; editing still works for this session */
    }
  }

  function selectAiPrompt() {
    aiPromptDraft = aiPromptBank.find((p) => p.id === aiPromptId)?.body ?? "";
  }

  function saveAiPromptDraft() {
    const i = aiPromptBank.findIndex((p) => p.id === aiPromptId);
    if (i < 0) return;
    aiPromptBank[i] = { ...aiPromptBank[i], body: aiPromptDraft };
    aiPromptBank = [...aiPromptBank];
    saveAiPrompts();
    showToast("Prompt sparad");
  }

  function resetAiPrompt() {
    const def = DEFAULT_AI_PROMPTS.find((p) => p.id === aiPromptId);
    aiPromptDraft = def?.body ?? "";
    const i = aiPromptBank.findIndex((p) => p.id === aiPromptId);
    if (i >= 0) {
      aiPromptBank[i] = { ...aiPromptBank[i], body: aiPromptDraft };
      aiPromptBank = [...aiPromptBank];
      saveAiPrompts();
    }
  }

  /** Open the "copy for AI" dialog for a text source, defaulting to the de-identified text. */
  async function openAiCopy(source: "anon" | "summary" | "transcript") {
    if (source === "anon" && reviewStale) { error = "Underlaget har ändrats. Kör om granskningen före kopiering."; return; }
    aiSource = source;
    aiUseOriginal = false;
    error = "";
    try {
      if (source === "anon") {
        aiText = await invoke<string>("copy_anonymized", { rejected: rejectedIds() });
        aiDeid = true;
      } else if (source === "summary") {
        aiText = summaryDraft;
        aiDeid = summaryAnonymized;
      } else {
        aiText = await summaryInputText();
        aiDeid = false;
      }
    } catch (e) {
      error = String(e);
      return;
    }
    if (!aiPromptBank.length) loadAiPrompts();
    if (!aiPromptBank.find((p) => p.id === aiPromptId)) aiPromptId = aiPromptBank[0].id;
    selectAiPrompt();
    aiOpen = true;
  }

  /** Assemble prompt + (pseudonym note) + context + the text, ready to paste into any AI. */
  function aiPayload(): string {
    const note = aiDeid
      ? "Texten nedan är avidentifierad: namn, platser m.m. är ersatta med pseudonymer (t.ex. Person 1, Plats 1) där samma pseudonym avser samma person/plats genomgående. Behåll pseudonymerna exakt — gissa eller hitta inte på riktiga identiteter."
      : "";
    const ctx = aiContext.trim() ? `Kontext: ${aiContext.trim()}` : "";
    const head = [aiPromptDraft.trim(), note, ctx].filter(Boolean).join("\n\n");
    return `${head}\n\n---\n\n${aiText}`;
  }

  async function copyForAi() {
    if (!aiDeid && !aiUseOriginal) return; // privacy gate: original text needs explicit confirmation
    try {
      await navigator.clipboard.writeText(aiPayload());
      showToast("Kopierat för AI");
      aiOpen = false;
    } catch (e) {
      error = String(e);
    }
  }

  /** Copy the payload, then open the chosen AI — prefilling its composer when the text is short
   *  enough, otherwise just opening the chat so the user pastes (Ctrl+V). */
  async function openInAi(target: { id: string; label: string; url: string; q: string | null }) {
    if (!aiDeid && !aiUseOriginal) return;
    const payload = aiPayload();
    try {
      await navigator.clipboard.writeText(payload);
    } catch {
      /* clipboard may be unavailable; we still open the site */
    }
    let url = target.url;
    let prefilled = false;
    if (target.q) {
      const enc = encodeURIComponent(payload);
      if (enc.length <= AI_PREFILL_MAX) {
        url = target.q + enc;
        prefilled = true;
      }
    }
    try {
      await openUrl(url);
      showToast(prefilled ? `Öppnar ${target.label}…` : `Kopierat – öppnar ${target.label}, klistra in (Ctrl+V)`);
      aiOpen = false;
    } catch (e) {
      error = "Kunde inte öppna webbläsaren: " + String(e);
    }
  }

  async function runSummarize() {
    if (!transcript || busy || qaBusy || actionsBusy) return;
    if (!summaryDownloaded) {
      error = "Hämta den valda sammanfattningsmodellen först.";
      return;
    }
    busy = true;
    error = "";
    progressMsg = "Förbereder…";
    try {
      if (!(await checkpointWork())) return;
      const anonymized = summaryFromAnon && !!analysis && !deidentDoc && !reviewStale;
      const text = await summaryInputText();
      summaryDraft = await invokeWork<string>("summarize", {
        args: { text, model: selectedSummaryModel, template: selectedTemplate, customHeadings },
      });
      summaryBasis = JSON.stringify({source:"transcript",text:transcript.utterances.map(u=>u.text).join("\n")});
      summaryAnonymized = anonymized;
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

  async function runTranscribe() {
    if (!audioPath || busy) return;
    if (!(await flushCurrentSave())) return;
    busy = true;
    error = "";
    transcribePct = 0;
    progressMsg = "Startar…";
    try {
      const t = await invokeWork<Transcript>("transcribe", {
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
      originalTranscript = null;
      undoStack = []; editingIdx = null;
      originalSourceText = null;
      summaryBasis = null;
      srcText = ""; srcPath = null; srcName = null; srcHasTables = false; srcMode = "transcript";
      currentJobPending = false;
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
      resetWorkspace();
      currentJobId = null; // a fresh transcription starts a new history entry
      currentJobTitle = "";
      currentCategory = lastCategory; // file it under the remembered folder by default
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
    snapshotTranscript();
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
    // Retain the review against its own source; reviewStale prevents exporting it as current.
    try {
      await invoke("update_transcript", { transcript });
      saveWorkspace();
    } catch (e) {
      error = String(e);
    }
  }

  /** Save a snapshot of the transcript before a mutation, so "Ångra" can restore it (cap 30). */
  function snapshotTranscript() {
    if (!transcript) return;
    undoStack = [...undoStack.slice(-29), JSON.parse(JSON.stringify(transcript)) as Transcript];
  }

  function undoEdit() {
    if (!undoStack.length) return;
    editingIdx = null;
    transcript = undoStack[undoStack.length - 1];
    undoStack = undoStack.slice(0, -1);
    void pushTranscript();
  }

  /** Remove an utterance (filler, cross-talk, mis-capture). */
  function deleteUtterance(idx: number) {
    if (!transcript) return;
    snapshotTranscript();
    editingIdx = null;
    transcript.utterances = transcript.utterances.filter((_, i) => i !== idx);
    void pushTranscript();
  }

  /** Reassign a single utterance to another speaker (`""` clears it). */
  function setSpeaker(idx: number, id: string) {
    if (!transcript) return;
    snapshotTranscript();
    transcript.utterances[idx] = { ...transcript.utterances[idx], speaker: id || null };
    void pushTranscript();
  }

  async function renameSpeaker(id: string, name: string) {
    speakerLabels[id] = name;
    saveWorkspace();
    // Labels live in the UI; nothing to push to the transcript itself.
  }

  /** Speaker display label for utterance/paragraph `para`, for the de-identify review (transcripts
   *  only; pasted text / documents have no speakers so this returns null). */
  function speakerForPara(para: number): string | null {
    const u = transcript?.utterances[para];
    if (!u?.speaker) return null;
    return speakerLabels[u.speaker] ?? u.speaker;
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
    if (busy || qaBusy || actionsBusy || recording || meetingActive || meetingBusy || !(await flushCurrentSave())) return;
    const path = await open({ multiple: false, filters: [{ name: "Avskrift-projekt", extensions: ["avskrift"] }] });
    if (typeof path !== "string") return;
    try {
      const p = await invoke<{ transcript: Transcript; speakerLabels: Record<string, string>; audioPath: string | null }>(
        "open_project",
        { path },
      );
      transcript = p.transcript;
      originalTranscript = null; originalSourceText = null; summaryBasis = null;
      undoStack = []; editingIdx = null; srcMode = "transcript";
      srcText = ""; srcPath = null; srcName = null; srcHasTables = false;
      meetingSysWav = null; meetingMicWav = null; meetingMixWav = null;
      currentJobId = null;
      currentJobTitle = "";
      currentJobCreatedAt = null;
      currentJobType = "transcribe";
      currentJobPending = false;
      resetWorkspace();
      speakerLabels = p.speakerLabels ?? {};
      audioPath = p.audioPath ?? null;
      audioName = audioPath ? audioPath.split(/[\\/]/).pop() ?? audioPath : "projekt";
      analysis = null;
      summaryDraft = "";
      dirty = false;
      view = "transcript";
      // Sync the backend transcript so export uses this project, not a stale one.
      await invoke("update_transcript", { transcript }).catch(() => {});
      await saveCurrentJob("transcribe");
      screen = "transcribe";
      showToast("Projektet öppnades");
    } catch (e) {
      error = String(e);
    }
  }

  async function runAnonymize() {
    if (!transcript || busy) return;
    if (!(await checkpointWork())) return;
    busy = true;
    error = "";
    progressMsg = "Avidentifierar…";
    try {
      const texts = transcript.utterances.map((u) => u.text);
      analysis = await invokeWork<AnalyzeResult>("anonymize", {
        args: { texts, enabled: ALL_KEYS, terms, useAi },
      });
      rejected = new Set();
      view = "review";
      deidentDoc = false;
      await saveCurrentJob(currentJobType);
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
    saveWorkspace();
  }

  function isActive(id: number): boolean {
    if (!analysis) return false;
    const span = analysis.spans[id];
    // Manual masks (the user clicked the word) always apply, regardless of category toggles.
    const allowed = span.source === "manual" || enabled[span.category];
    return allowed && !rejected.has(id);
  }
  const activeCount = $derived(analysis ? analysis.spans.filter((s) => isActive(s.id)).length : 0);
  function countFor(key: string): number {
    return analysis ? analysis.spans.filter((s) => s.category === key).length : 0;
  }
  function toggleSpan(id: number) {
    if (!analysis) return;
    const span = analysis.spans[id];
    if (span.source !== "manual" && !enabled[span.category]) return;
    const next = new Set(rejected);
    next.has(id) ? next.delete(id) : next.add(id);
    rejected = next;
    saveWorkspace();
  }
  function rejectedIds(): number[] {
    return analysis ? analysis.spans.filter((s) => !isActive(s.id)).map((s) => s.id) : [];
  }

  // ---- Manual masking: click an undetected word in the review to mask it ----
  let maskTarget = $state<{ start: number; end: number; text: string } | null>(null);
  let maskCategory = $state("ovrigt");
  let maskCustom = $state("");
  // Pending drag-selection (byte range + viewport position for the floating "mask selection" button).
  let selPending = $state<{ start: number; end: number; text: string; x: number; y: number } | null>(null);
  function openMask(seg: { start: number; end: number; text: string }) {
    selPending = null;
    maskTarget = { start: seg.start, end: seg.end, text: seg.text };
    maskCategory = "ovrigt";
    maskCustom = "";
  }

  // After a drag-selection in the review document, map it to a byte range in `analysis.text` and
  // offer a floating "mask selection" button. Relies on the rendered segments concatenating exactly
  // to analysis.text (each segment renders its verbatim substring), so the document's text content
  // equals the analysed text and a Range's byte length gives the offset.
  function onDocMouseUp(e: MouseEvent) {
    const sel = window.getSelection();
    if (!sel || sel.isCollapsed || !analysis) { selPending = null; return; }
    const range = sel.getRangeAt(0);
    const doc = e.currentTarget as HTMLElement;
    if (!doc.contains(range.startContainer) || !doc.contains(range.endContainer)) { selPending = null; return; }
    const raw = sel.toString();
    const trimmed = raw.trim();
    if (!trimmed) { selPending = null; return; }
    const enc = new TextEncoder();
    const pre = range.cloneRange();
    pre.selectNodeContents(doc);
    pre.setEnd(range.startContainer, range.startOffset);
    const start = enc.encode(pre.toString()).length + enc.encode(raw.slice(0, raw.indexOf(trimmed))).length;
    const end = start + enc.encode(trimmed).length;
    const r = range.getBoundingClientRect();
    selPending = { start, end, text: trimmed, x: r.left + r.width / 2, y: r.top };
  }
  function maskSelection() {
    if (!selPending) return;
    openMask({ start: selPending.start, end: selPending.end, text: selPending.text });
    window.getSelection()?.removeAllRanges();
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
      saveWorkspace();
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

  async function copyAnon() {
    if (reviewStale) { error="Underlaget har ändrats. Kör om granskningen före export."; return; }
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

  /** Copy the raw transcript (with speaker labels) to the clipboard. */
  async function copyTranscript() {
    if (!transcript) return;
    const text = transcript.utterances
      .map((u) => {
        const name = u.speaker ? speakerLabels[u.speaker] ?? u.speaker : null;
        return name ? `${name}: ${u.text}` : u.text;
      })
      .join("\n");
    try {
      await navigator.clipboard.writeText(text);
      showToast("Kopierat till urklipp");
    } catch (e) {
      error = String(e);
    }
  }

  async function startRecording() {
    if (recording || busy || qaBusy || actionsBusy || meetingActive || meetingBusy) return;
    if (dictation && dictation.phase !== "idle") {
      error = "Stoppa diktatet innan du startar en ljudinspelning.";
      return;
    }
    error = "";
    await newProject();
    if (currentJobId || saveState === "error") return;
    screen = "transcribe";
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
    if (recNode) recNode.onaudioprocess = null; // stop accumulating immediately
    recNode?.disconnect();
    recStream?.getTracks().forEach((t) => t.stop());
    await recCtx?.close();

    // Flatten captured chunks, downsample to 16 kHz, encode a 16-bit PCM WAV (matches the pipeline's
    // target rate).
    const total = recChunks.reduce((n, c) => n + c.length, 0);
    if (total === 0) {
      recChunks = [];
      recCtx = recNode = recStream = null;
      error = "Ingen ljud spelades in – kontrollera att mikrofonen fungerar och har behörighet.";
      return;
    }
    recSaving = true;
    error = "";
    try {
      const pcm = new Float32Array(total);
      let off = 0;
      for (const c of recChunks) { pcm.set(c, off); off += c.length; }
      const down = downsampleTo16k(pcm, recSampleRate);
      const wav = encodeWav(down, 16000);
      recChunks = [];
      recCtx = recNode = recStream = null;
      // Send the WAV as a base64 string, NOT Array.from(new Uint8Array(...)). The latter becomes a
      // JS number-per-byte array that Tauri JSON-serializes over IPC — millions of elements for a
      // few minutes of audio, which froze the app on stop and never saved. base64 is one compact
      // string the backend decodes on a blocking thread.
      const b64 = bytesToBase64(new Uint8Array(wav));
      const path = await invoke<string>("save_recording", { data: b64 });
      audioPath = path;
      audioName = `Inspelning (${fmtTime(recElapsed)})`;
      transcript = null;
      analysis = null;
      showToast("Inspelning klar");
    } catch (e) {
      error = "Kunde inte spara inspelningen: " + String(e);
    } finally {
      recSaving = false;
    }
  }

  /** Base64-encode bytes in chunks (a single String.fromCharCode(...wholeArray) overflows the call
   *  stack on large buffers). */
  function bytesToBase64(bytes: Uint8Array): string {
    let binary = "";
    const chunk = 0x8000;
    for (let i = 0; i < bytes.length; i += chunk) {
      binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
    }
    return btoa(binary);
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
    if (soundTestBusy || meetingActive || meetingBusy || busy || qaBusy || actionsBusy) return;
    if (!(await flushCurrentSave())) return;
    const id=crypto.randomUUID(), now=new Date().toISOString();
    const title=meetingName.trim()||`Möte ${fmtJobDate(now)}`;
    meetingBusy=true;error='';
    try {
      const job={version:2,id,jobType:'meeting',title,createdAt:now,updatedAt:now,category:currentCategory||lastCategory,
        notes:'',agenda:meetingAgenda,participants:meetingPeople.split("\n").filter(line=>line.trim()).map(line=>{const [name,...roles]=line.split(";");return {name:name.trim(),role:roles.join(";").trim()};}),summaryTemplate:selectedTemplate,actions:followupActions.filter(a=>!excludedFollowup.includes(a.id!)),followupFrom,transcriptionPending:true};
      const ack=await invoke<{micWavPath:string;systemWavPath:string}>('start_meeting',{args:{model:selectedModel,language,live:meetingLive,job,micDevice:micDevice||null,systemDevice:systemDevice||null}});
      resetWorkspace();
      transcript={utterances:[],language,model:selectedModel,diarized:true};originalTranscript=null;originalSourceText=null;summaryBasis=null;
      undoStack=[];editingIdx=null;analysis=null;summaryDraft='';srcText='';srcPath=null;srcName=null;srcHasTables=false;
      audioPath=ack.systemWavPath;audioName=title;meetingSysWav=ack.systemWavPath;meetingMicWav=ack.micWavPath;meetingMixWav=null;
      currentJobId=id;currentJobTitle=title;currentJobCreatedAt=now;currentJobType='meeting';currentJobPending=true;
      agenda=meetingAgenda;actions=job.actions;participants=job.participants;followupFrom=job.followupFrom;meetingWarning='';channelLevels=[];meetingLagging=false;
      meetingShowLive=true;meetingShowNotes=true;meetingActive=true;meetingElapsed=0;liveUtterances=[];saveState='saved';savedAt=now;dirty=false;
      meetingTimer=setInterval(()=>(meetingElapsed+=1),1000);
      meetingName='';meetingAgenda='';meetingPeople='';followupActions=[];
      void refreshJobs();
    }catch(e){error='Kunde inte starta inspelningen: '+String(e);}
    finally{meetingBusy=false;}
  }

  async function stopMeeting() {
    if(!meetingActive||meetingBusy||!currentJobId)return;
    meetingBusy=true;
    await flushCurrentSave();
    const token=currentJobId;
    bgMeetings=[...bgMeetings,{id:token,title:currentJobTitle,msg:'Transkriberar…'}];
    try {
      await invoke('stop_meeting',{args:{model:selectedModel,language,token}});
      meetingActive=false;if(meetingTimer)clearInterval(meetingTimer);
      view='overview';screen='transcribe';
      showToast('Inspelningen är sparad. Du kan fortsätta anteckna medan transkriptet blir klart.');
    }catch(e){error=String(e);bgMeetings=bgMeetings.filter(m=>m.id!==token);}
    finally{meetingBusy=false;transcribePct=null;progressMsg='';void refreshJobs();}
  }

  async function onMeetingDone(payload:any) {
    const token=payload?.token;if(!token)return;
    bgMeetings=bgMeetings.filter(m=>m.id!==token);
    try {
      // Native finalisation atomically updates only audio/text. It cannot overwrite notes or names.
      if(currentJobId===token){
        const job=await invoke<any>('open_job',{id:token});
        if(currentJobId!==token)return;
        transcript=payload.transcript;currentJobPending=false;meetingMixWav=payload.mixWavPath??null;
        originalTranscript=job.originalTranscript??payload.transcript;
        meetingWarning=job.meetingWarning??job.meetingError??'';
        speakerLabels=Object.fromEntries((transcript?.utterances??[]).filter(u=>u.speaker).map(u=>[u.speaker!,u.speaker!]));
        await invoke('update_transcript',{transcript});
      }
      await refreshJobs();await loadAllActions();showToast('Mötets transkript är klart.');
    }catch(e){error='Kunde inte uppdatera mötesvyn: '+String(e);}
  }

  async function askMeeting(){
    if(!qaQuestion.trim()||!transcript||busy||qaBusy||actionsBusy)return;
    const question=qaQuestion.trim(),id=currentJobId;
    qaBusy=true;error='';
    try{const text=await summaryInputText();const answer=await invokeWork<string>('ask_transcript',{args:{question,transcriptText:text,model:selectedSummaryModel}});if(currentJobId===id){qaHistory=[...qaHistory,{q:question,a:answer}];qaQuestion='';}}
    catch(e){error=String(e);}finally{qaBusy=false;progressMsg='';}
  }

  /** Split the meeting's "Mötet" utterances into distinct speakers via diarisation of the system WAV. */
  async function separateMeetingVoices() {
    if (!meetingSysWav || !transcript || busy) return;
    const jobId = currentJobId;
    const sys = meetingSysWav;
    busy = true;
    error = "";
    progressMsg = "Separerar mötesröster…";
    try {
      const t = await invoke<Transcript>("diarize_meeting", {
        args: { systemWavPath: sys, numSpeakers: null },
      });
      if (currentJobId !== jobId) {
        showToast("Avbröts – du bytte projekt under tiden.");
        return;
      }
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

  /** Re-run Whisper on the saved meeting WAVs with the chosen model — a full batch pass, usually
   *  better than the live result (pick a larger model for best quality). Resets to Jag/Mötet. */
  async function retranscribeMeeting() {
    if (!meetingMicWav || !meetingSysWav || busy || !selectedDownloaded) return;
    if (!(await checkpointWork())) return;
    // Capture which meeting we're working on. This is a multi-minute async job; if the user opens a
    // different project meanwhile, the result must NOT be saved onto that other project (which would
    // swap transcripts between meetings). Abort instead.
    const jobId = currentJobId;
    const sys = meetingSysWav;
    const mic = meetingMicWav;
    busy = true;
    error = "";
    transcribePct = 0;
    progressMsg = "Transkriberar om mötet…";
    try {
      const result = await invoke<{ transcript: Transcript; mixWavPath: string | null }>("retranscribe_meeting", {
        args: { micWavPath: mic, systemWavPath: sys, model: selectedModel, language, echoCancel: retranscribeEchoCancel },
      });
      if (currentJobId !== jobId) {
        showToast("Omtranskriberingen avbröts – du bytte projekt under tiden. Öppna mötet och kör om.");
        return;
      }
      let t = result.transcript;
      meetingMixWav = result.mixWavPath ?? null; // refreshed playback mix (matches echo-cancel choice)
      if (retranscribeDiarize && sys) {
        progressMsg = "Separerar mötesröster…";
        t = await invoke<Transcript>("diarize_meeting", {
          args: { systemWavPath: sys, numSpeakers: null },
        });
        if (currentJobId !== jobId) {
          showToast("Avbröts – du bytte projekt under tiden. Öppna mötet och kör om.");
          return;
        }
      }
      transcript = t;
      // Map speaker ids to friendly labels (Jag/Mötet kept as-is; diarised TALARE_n → "Talare n").
      const labels: Record<string, string> = {};
      for (const u of t.utterances) {
        if (u.speaker && !(u.speaker in labels)) {
          labels[u.speaker] = u.speaker.startsWith("TALARE_") ? u.speaker.replace("TALARE_", "Talare ") : u.speaker;
        }
      }
      speakerLabels = labels;
      analysis = null;
      summaryDraft = "";
      dirty = false;
      undoStack = [];
      editingIdx = null;
      currentJobPending = false; // now it has a transcript — clear the "unfinished" banner/badge
      meetingWarning=t.utterances.some(u=>u.speaker==='Jag')?'':'Ingen mikrofontext hittades. Lyssna på Min mikrofon och kontrollera inspelningen.';
      await saveCurrentJob("meeting");
      showToast(retranscribeDiarize ? "Mötet omtranskriberat · röster separerade" : "Mötet transkriberat om");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      transcribePct = null;
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

  /** Distinct speaker ids in the transcript, for the per-utterance "byt talare" dropdown. */
  const speakerOptions = $derived.by(() => {
    const seen: string[] = [];
    for (const u of transcript?.utterances ?? []) if (u.speaker && !seen.includes(u.speaker)) seen.push(u.speaker);
    return seen;
  });

  // ============================================================================
  // Task-oriented screens, standalone de-identify/summarize, and jobs history
  // ============================================================================
  type Screen = "home" | "transcribe" | "meeting" | "deidentify" | "summarize" | "history" | "tasks" | "dictation";
  let dictation = $state<DictationSnapshot | null>(null);
  function updateDictation(next: DictationSnapshot) {
    if (!dictation || next.revision >= dictation.revision) dictation = next;
  }
  onMount(() => {
    let disposed = false;
    const event = listen<DictationSnapshot>("avskrift:dictation", e => updateDictation(e.payload));
    const close = listen("avskrift:dictation-close-blocked", () => {
      screen = "dictation";
      error = "Stoppa eller avbryt diktatet innan du stänger AVskrift.";
    });
    void event.then(() => invoke<DictationSnapshot>("dictation_snapshot")).then(s => {
      if (!disposed) updateDictation(s);
    }).catch(e => { if (!disposed) error = String(e); });
    return () => { disposed = true; void event.then(f => f()); void close.then(f => f()); };
  });
  let screen = $state<Screen>("home");
  const transcriptReading = $derived(screen === "transcribe" && !!transcript && view === "transcript");
  const controlsCollapsed = $derived(["overview","notes","actions"].includes(view)&&screen==="transcribe" ? true : transcriptReading ? !transcriptToolsOpen : sidebarCollapsed);

  function go(s: Screen) {
    if(currentJobId&&!meetingActive&&screen==="transcribe")saveWorkspace();
    screen = s;
    error = "";
    if (s === "history") {
      selectedFolder = "";
      jobSearch = "";
      void refreshJobs();
      void searchJobs();
    }
    // The Åtaganden overview and the Home card both read the cross-project action list — keep it fresh.
    if (s === "tasks" || s === "home") {void loadAllActions();void refreshJobs();}
  }

  /** Switch to a tab of the current transcript workspace (driven by the context-aware top nav). */
  function tab(v: "transcript" | "review" | "summary" | "qa" | "notes" | "overview" | "actions") {
    view = v;
    if(currentJobId)saveWorkspace();
    screen = "transcribe";
    error = "";
  }

  /** Clear the current working project and return Home for a fresh start. Everything is auto-saved
   *  in Historik, so nothing is lost — reopen a job there to resume it. */
  async function newProject() {
    // Don't wipe a capture/job that's still running; just navigate home.
    if (recording || meetingActive || meetingBusy || busy || qaBusy || actionsBusy) {
      go("home");
      return;
    }
    if (!(await flushCurrentSave())) return;
    saveState = "idle";
    saveError = "";
    savedAt = "";
    originalTranscript = null;
    originalSourceText = null;
    undoStack = [];
    summaryBasis = null;
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
    meetingMixWav = null;
    liveUtterances = [];
    currentJobId = null;
    currentJobPending = false;
    currentJobCreatedAt = null;
    currentJobTitle = "";
    currentJobType = "transcribe";
    currentCategory = lastCategory;
    resetWorkspace();
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
      saveWorkspace();
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
    saveWorkspace();
  }

  // ---- Standalone de-identify (pasted text or a loaded doc; sets engine.last) ----
  async function runDeidentify() {
    if (busy || qaBusy || actionsBusy) return;
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
    if (!(await checkpointWork())) return;
    if (originalSourceText === null) originalSourceText = srcText;
    busy = true;
    error = "";
    progressMsg = "Avidentifierar…";
    try {
      analysis = await invokeWork<AnalyzeResult>("analyze_document", {
        args: {
          text: srcText,
          path: null,
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
    if (reviewStale) { error="Underlaget har ändrats. Kör om granskningen före export."; return; }
    try {
      const text = await invoke<string>("copy_anonymized", { rejected: rejectedIds() });
      await navigator.clipboard.writeText(text);
      showToast("Kopierat till urklipp");
    } catch (e) {
      error = String(e);
    }
  }
  // ---- Standalone summarize ----
  async function doSummarize(text: string, anonymized = false) {
    if (busy || qaBusy || actionsBusy) return;
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
      if (!(await checkpointWork())) return;
      if (srcMode !== "transcript" && originalSourceText === null) originalSourceText = srcText;
      const basis = JSON.stringify({source:srcMode === "transcript" ? "transcript" : "document",text:srcMode === "transcript" ? transcript?.utterances.map(u=>u.text).join("\n") : srcText});
      summaryDraft = await invokeWork<string>("summarize", {
        args: { text, model: selectedSummaryModel, template: selectedTemplate, customHeadings },
      });
      summaryBasis = basis;
      summaryAnonymized = anonymized;
      view = "summary";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      progressMsg = "";
    }
  }
  async function runSummarizeSource() {
    if (busy || qaBusy || actionsBusy) return;
    try {
      const anonymized = srcMode === 'transcript' && summaryFromAnon && !!analysis && !deidentDoc && !reviewStale;
      const text = srcMode === "transcript" ? await summaryInputText() : srcText;
      await doSummarize(text, anonymized);
      if (summaryDraft) await saveCurrentJob("summarize");
    } catch(e) { error = String(e); }
  }

  // ---- Jobs / history (auto-saved past work) ----
  type JobMeta = { id: string; title: string; jobType: string; category: string; createdAt: string; updatedAt: string; audioBytes: number; actionsTotal: number; actionsDone: number; hasNotes: boolean; transcriptionPending: boolean; pinned?:boolean; archived?:boolean; lastOpened?:string; followup?:string };
  let allJobs = $state<JobMeta[]>([]);
  let recentJobs = $state<JobMeta[]>([]);
  let historyJobs = $state<JobMeta[]>([]); // what the History screen shows (search-filtered)
  let jobSearch = $state(""); // full-text query for History
  let renamingJobId = $state<string | null>(null);
  let renameText = $state("");
  let selectedFolder = $state(""); // selected folder in the History tree ("" = Alla / root)
  let expandedFolders = $state<string[]>([]); // expanded tree nodes (paths)
  let pendingFolders = $state<string[]>([]); // created-but-empty folders (the path model can't store these in jobs)
  let selectedJobIds = $state<string[]>([]); // multi-select for bulk actions in History
  let dragging = $state<{ id: string; label: string } | null>(null); // pointer-drag of a project
  let dragX = $state(0);
  let dragY = $state(0);
  let dropTarget = $state<string | null>(null); // folder path currently hovered as a drop target ("" = root)
  let renamingFolder = $state<string | null>(null);
  let folderRenameText = $state("");
  let ctxMenu = $state<{ x: number; y: number; kind: "folder" | "job"; target: string } | null>(null);
  let currentCategory = $state(""); // folder of the active job (path, "/"-separated)
  let lastCategory = $state(""); // remembered folder, applied to the next new job
  let folderPickerFor = $state<string | null>(null); // which folder dropdown is open ("header" | job id | null)
  let newFolderName = $state(""); // create-folder input inside the dropdown
  let currentJobId = $state<string | null>(null);
  let currentJobPending = $state(false); // opened a meeting whose transcription is unfinished (recoverable)
  let currentJobCreatedAt = $state<string | null>(null);
  let currentJobTitle = $state(""); // stable title for the active job (set once; survives re-transcribe/reopen)
  let currentJobType = $state<"transcribe" | "meeting" | "deidentify" | "summarize">("transcribe");

  // ---- Meeting workspace (B): notes, participants, actions, follow-up ----
  type ActionItem = { source?: {quote:string;start:number|null;jobId?:string}; id?:string; text: string; done: boolean; assignee: string; due: string };
  let meetingName = $state("");
  let meetingAgenda = $state("");
  let agenda = $state("");
  let bookmarks = $state<{id:string;time:number;text:string}[]>([]);
  let decisions = $state<{id:string;text:string;quote?:string;start?:number|null}[]>([]);
  let decisionText = $state("");
  let meetingWarning = $state("");
  let playbackRate = $state(1);
  let devices = $state<{id:string;name:string;source:string}[]>([]);
  let micDevice = $state(""); let systemDevice = $state("");
  let channelLevels = $state<{name:string;peak:number;active:boolean;error?:string}[]>([]);
  let titleInput=$state<HTMLInputElement>();
  let titleEditing = $state(false); let titleDraft = $state("");
  let renameTarget = $state<JobMeta|null>(null);
  let renameBusy = $state(false);
  let libraryFilter = $state("active"); let libraryType = $state("");
  let meetingPreset = $state("");
  let presetName = $state("");
  let meetingPresets = $state<{name:string;agenda:string;people?:string;summaryTemplate?:string}[]>([]);
  let meetingPeople=$state("");
  let followupFrom = $state<string|null>(null);
  let followupActions = $state<ActionItem[]>([]);
  let excludedFollowup = $state<string[]>([]);
  let meetingDevicesError = $state("");
  let soundTestBusy = $state(false); let soundTestResult = $state('');
  async function testMeetingSound(){if(soundTestBusy||meetingActive)return;soundTestBusy=true;soundTestResult='Tala nu och spela upp ljud från mötet. Mäter i tre sekunder…';try{const levels=await invoke<typeof channelLevels>('test_meeting_audio',{micDevice:micDevice||null,systemDevice:systemDevice||null});soundTestResult=levels.map((l,i)=>(i===0?'Mikrofon: ':'Mötesljud: ')+(l.peak>0.0003?'ljud registrerat':'ingen nivå registrerad')).join('. ')+'. Testet mäter ljudnivå, inte taligenkänning.';}catch(e){soundTestResult=String(e);}finally{soundTestBusy=false;}}
  const listedJobs = $derived(historyJobs.filter(j=>(libraryFilter==='archived'?j.archived:!j.archived) && (libraryFilter!=='pinned'||j.pinned) && (!libraryType||j.jobType===libraryType)));
  const continuedJobs = $derived([...allJobs.filter(j=>!j.archived)].sort((a,b)=>Number(!!b.pinned)-Number(!!a.pinned)||(b.lastOpened||b.updatedAt).localeCompare(a.lastOpened||a.updatedAt)).slice(0,6));
  const followupJobs = $derived(allJobs.filter(j=>!j.archived&&j.followup).sort((a,b)=>(a.followup||'').localeCompare(b.followup||'')).slice(0,5));
  async function loadMeetingDevices(){try{const result=await invoke<typeof devices>('meeting_devices');devices=Array.isArray(result)?result:[];meetingDevicesError='';}catch(e){meetingDevicesError=String(e);}}
  $effect(()=>{if(screen==='meeting'&&!meetingActive)void loadMeetingDevices();});
  $effect(()=>{
    if(!meetingActive)return;
    let stopped=false,reading=false;
    const timer=setInterval(async()=>{if(reading)return;reading=true;try{const next=await invoke<typeof channelLevels>('meeting_levels');if(!stopped){channelLevels=next;const fault=next.find(l=>l.error)?.error;if(fault)meetingWarning=fault;}}catch{}finally{reading=false;}},300);
    return ()=>{stopped=true;clearInterval(timer);};
  });
  onMount(()=>{try{meetingPresets=JSON.parse(localStorage.getItem('avskrift.meetingPresets')||'[]');}catch{}});
  function saveMeetingPreset(){if(!presetName.trim())return;meetingPresets=[...meetingPresets.filter(p=>p.name!==presetName.trim()),{name:presetName.trim(),agenda:meetingAgenda,people:meetingPeople,summaryTemplate:selectedTemplate}];localStorage.setItem('avskrift.meetingPresets',JSON.stringify(meetingPresets));meetingPreset=presetName.trim();presetName='';}
  async function listenActionSource(source:{quote:string;start:number|null;jobId?:string}){if(source.jobId&&source.jobId!==currentJobId){await openJobById(source.jobId);if(currentJobId!==source.jobId)return;}tab('transcript');await tick();if(source.start!==null)seekTo(source.start);}
  function markMeetingTime(){bookmarks=[...bookmarks,{id:crypto.randomUUID(),time:meetingActive?meetingElapsed:currentTime,text:'Markering'}];saveWorkspace();}
  function addDecision(){if(!decisionText.trim())return;decisions=[...decisions,{id:crypto.randomUUID(),text:decisionText.trim()}];decisionText='';saveWorkspace();}
  function addGroundedDecision(text:string,source:{quote:string;start:number|null}){decisions=[...decisions,{id:crypto.randomUUID(),text,...source}];saveWorkspace();}
  async function prepareFollowup(){if(!(await flushCurrentSave()))return;meetingName='Uppföljning: '+currentJobTitle;meetingAgenda=agenda;meetingPeople=participants.map(p=>p.name+(p.role?'; '+p.role:'')).join('\n');followupActions=actions.filter(a=>!a.done).map(a=>({...a,id:crypto.randomUUID(),source:a.source?{...a.source,jobId:a.source.jobId??currentJobId??undefined}:undefined}));excludedFollowup=[];followupFrom=currentJobId;go('meeting');}
  async function organize(j:JobMeta,kind:'pinned'|'archived'){try{await invoke('organize_job',{id:j.id,[kind]:!j[kind]});await reloadJobs();}catch(e){error=String(e);}}
  async function editTitle(j?:JobMeta){renameTarget=j??null;titleDraft=j?.title??currentJobTitle;titleEditing=true;await tick();titleInput?.focus();titleInput?.select();}
  async function saveTitle(){
    const title=titleDraft.trim();if(!title||renameBusy)return;renameBusy=true;
    try{
      if(!renameTarget||renameTarget.id===currentJobId){currentJobTitle=title;editRevision++;if(!(await saveCurrentJob(currentJobType)))return;}
      else await invoke('update_job_meta',{id:renameTarget.id,title,category:renameTarget.category});
      titleEditing=false;await reloadJobs();
    }catch(e){error=String(e);}finally{renameBusy=false;}
  }
  let notes = $state("");
  let participants = $state<{ name: string; role: string }[]>([]);
  let actions = $state<ActionItem[]>([]);
  let followup = $state("");
  let newActionText = $state("");
  let actionPasteOpen = $state(false);
  let actionPasteText = $state("");
  let maximized = $state<null | "notes" | "actions">(null);
  const wsHasContent = $derived(
    !!(notes.trim() || followup.trim() || actions.length || participants.some((p) => p.name.trim() || p.role.trim())),
  );

  // ============================================================================
  // Åtaganden — cross-project action-item overview (decoupled from any one project)
  // ============================================================================
  type ActionRow = {
    source: "job" | "standalone";
    jobId: string;
    jobTitle: string;
    jobType: string;
    category: string;
    taskId: string;
    index: number;
    text: string;
    done: boolean;
    assignee: string;
    due: string;
    createdAt: string;
    updatedAt: string;
  };
  let allActions = $state<ActionRow[]>([]);
  let taskStatus = $state<"open" | "done" | "all">("open");
  let taskQuery = $state("");
  let taskAssignee = $state(""); // "" = alla ansvariga
  let taskFolder = $state(""); // "" = alla mappar (prefix-match → undermappar inräknade)
  let taskDue = $state<"all" | "overdue" | "week" | "none">("all");
  let taskSort = $state<"due" | "updated" | "project">("due");
  let taskGroup = $state<"flat" | "project" | "assignee">("flat");
  let taskBusy = $state(false);
  // Add-form
  let newTaskText = $state("");
  let newTaskAssignee = $state("");
  let newTaskDue = $state("");
  let newTaskTarget = $state("standalone"); // "standalone" | "folder:<path>" | "job:<id>"

  async function loadAllActions() {
    try {
      allActions = await invoke<ActionRow[]>("list_all_actions");
    } catch {
      /* non-fatal */
    }
  }

  function isISODate(s: string): boolean {
    return /^\d{4}-\d{2}-\d{2}$/.test((s || "").trim());
  }
  function todayISO(): string {
    const d = new Date();
    const m = String(d.getMonth() + 1).padStart(2, "0");
    const day = String(d.getDate()).padStart(2, "0");
    return `${d.getFullYear()}-${m}-${day}`;
  }
  /** Classify a due value. Only ISO dates (YYYY-MM-DD) are interpreted; free text ("fredag") → "text". */
  function dueState(due: string): "none" | "text" | "overdue" | "today" | "soon" | "future" {
    const t = (due || "").trim();
    if (!t) return "none";
    if (!isISODate(t)) return "text";
    const today = todayISO();
    if (t < today) return "overdue";
    if (t === today) return "today";
    const diff = (new Date(t + "T00:00:00").getTime() - new Date(today + "T00:00:00").getTime()) / 86400000;
    return diff <= 7 ? "soon" : "future";
  }
  function projName(a: ActionRow): string {
    return a.source === "standalone" ? "Fristående" : a.jobTitle || "Namnlöst";
  }
  function rowKey(a: ActionRow): string {
    return a.source === "standalone" ? "t:" + a.taskId : "j:" + a.jobId + ":" + a.index;
  }
  // Sort key: not-done before done, then dated (ascending) before undated.
  function dueSortKey(a: ActionRow): string {
    const t = (a.due || "").trim();
    return (a.done ? "1" : "0") + (isISODate(t) ? "1" + t : "2");
  }

  const taskAssignees = $derived.by(() => {
    const s = new Set<string>();
    for (const a of allActions) {
      const v = a.assignee.trim();
      if (v) s.add(v);
    }
    return [...s].sort((x, y) => x.localeCompare(y, "sv"));
  });

  // Folders that actually contain åtaganden (jobs OR loose tasks) — drives the filter menu.
  const taskFolders = $derived.by(() => {
    const s = new Set<string>();
    for (const a of allActions) {
      const cat = (a.category || "").trim().replace(/^\/+|\/+$/g, "");
      if (!cat) continue;
      const parts = cat.split("/");
      for (let i = 1; i <= parts.length; i++) s.add(parts.slice(0, i).join("/")); // include ancestors
    }
    return [...s].sort((x, y) => x.localeCompare(y, "sv"));
  });

  // Folders you can file a new task into — every known project folder, plus any that already hold tasks.
  const addFolders = $derived.by(() => {
    const s = new Set<string>();
    for (const f of allFolders) s.add(f.path);
    for (const f of taskFolders) s.add(f);
    return [...s].sort((x, y) => x.localeCompare(y, "sv"));
  });

  const taskCounts = $derived.by(() => {
    let open = 0,
      overdue = 0,
      done = 0;
    for (const a of allActions) {
      if (a.done) {
        done++;
        continue;
      }
      open++;
      if (dueState(a.due) === "overdue") overdue++;
    }
    return { open, overdue, done, total: allActions.length };
  });

  const filteredActions = $derived.by(() => {
    const q = taskQuery.trim().toLowerCase();
    const rows = allActions.filter((a) => {
      if (taskStatus === "open" && a.done) return false;
      if (taskStatus === "done" && !a.done) return false;
      if (taskAssignee && a.assignee.trim() !== taskAssignee) return false;
      if (taskFolder) {
        const cat = (a.category || "").trim().replace(/^\/+|\/+$/g, "");
        if (cat !== taskFolder && !cat.startsWith(taskFolder + "/")) return false;
      }
      if (taskDue !== "all") {
        const ds = dueState(a.due);
        if (taskDue === "overdue" && ds !== "overdue") return false;
        if (taskDue === "week" && ds !== "today" && ds !== "soon") return false;
        if (taskDue === "none" && ds !== "none" && ds !== "text") return false;
      }
      if (q && !(a.text + " " + a.assignee + " " + a.jobTitle).toLowerCase().includes(q)) return false;
      return true;
    });
    rows.sort((a, b) => {
      if (taskSort === "updated") return (b.updatedAt || "").localeCompare(a.updatedAt || "");
      if (taskSort === "project") {
        const c = projName(a).localeCompare(projName(b), "sv");
        return c !== 0 ? c : dueSortKey(a).localeCompare(dueSortKey(b));
      }
      const c = dueSortKey(a).localeCompare(dueSortKey(b));
      return c !== 0 ? c : projName(a).localeCompare(projName(b), "sv");
    });
    return rows;
  });

  const groupedActions = $derived.by(() => {
    if (taskGroup === "flat") return [{ key: "", rows: filteredActions }];
    const map = new Map<string, ActionRow[]>();
    for (const a of filteredActions) {
      const key = taskGroup === "project" ? projName(a) : a.assignee.trim() || "Utan ansvarig";
      const arr = map.get(key);
      if (arr) arr.push(a);
      else map.set(key, [a]);
    }
    return [...map.entries()].sort((x, y) => x[0].localeCompare(y[0], "sv")).map(([key, rows]) => ({ key, rows }));
  });

  /** Write a change back to the owning job/task, then refresh the overview and the project list. */
  async function updateRow(a: ActionRow, patch: Partial<Pick<ActionRow, "text" | "done" | "assignee" | "due">>) {
    if (patch.text !== undefined && !patch.text.trim()) {
      await loadAllActions(); // empty edit → restore the displayed value, don't persist a blank
      return;
    }
    const next = { text: a.text, done: a.done, assignee: a.assignee, due: a.due, ...patch };
    const now = new Date().toISOString();
    try {
      if (a.source === "standalone") {
        await invoke("update_standalone_task", {
          args: { task: { id: a.taskId, category: a.category, ...next, createdAt: a.createdAt || now, updatedAt: now } },
        });
      } else {
        await invoke("update_job_action", { args: { jobId: a.jobId, index: a.index, action: next, updatedAt: now } });
        if (currentJobId === a.jobId) actions = actions.map((x, k) => (k === a.index ? { ...x, ...next } : x));
      }
      await loadAllActions();
      await refreshJobs();
    } catch (e) {
      error = String(e);
    }
  }

  async function deleteRow(a: ActionRow) {
    const now = new Date().toISOString();
    try {
      if (a.source === "standalone") {
        await invoke("delete_standalone_task", { id: a.taskId });
      } else {
        await invoke("delete_job_action", { args: { jobId: a.jobId, index: a.index, updatedAt: now } });
        if (currentJobId === a.jobId) actions = actions.filter((_, k) => k !== a.index);
      }
      await loadAllActions();
      await refreshJobs();
    } catch (e) {
      error = String(e);
    }
  }

  async function addTask() {
    const text = newTaskText.trim();
    if (!text || taskBusy) return;
    taskBusy = true;
    const now = new Date().toISOString();
    const assignee = newTaskAssignee.trim();
    const due = newTaskDue.trim();
    try {
      if (newTaskTarget.startsWith("job:")) {
        const jobId = newTaskTarget.slice(4);
        await invoke("add_job_action", {
          args: { jobId, action: { text, done: false, assignee, due }, updatedAt: now },
        });
        if (currentJobId === jobId) actions = [...actions, { text, done: false, assignee, due }];
      } else {
        // "standalone" → no folder · "folder:<path>" → a loose task filed in that folder
        const category = newTaskTarget.startsWith("folder:") ? newTaskTarget.slice(7) : "";
        const id = crypto.randomUUID ? crypto.randomUUID() : String(Date.now());
        await invoke("add_standalone_task", {
          args: { task: { id, text, done: false, assignee, due, category, createdAt: now, updatedAt: now } },
        });
      }
      newTaskText = "";
      newTaskAssignee = "";
      newTaskDue = "";
      await loadAllActions();
      await refreshJobs();
    } catch (e) {
      error = String(e);
    } finally {
      taskBusy = false;
    }
  }

  /** Jump from an åtagande to its source project's notes/actions tab. */
  function openRowProject(a: ActionRow) {
    if (a.source !== "job" || !a.jobId) return;
    void openJobById(a.jobId).then(() => {
      view = "notes";
    });
  }

  // When you filter to a folder, new tasks default into that folder (until you pick another target).
  $effect(() => {
    newTaskTarget = taskFolder ? "folder:" + taskFolder : "standalone";
  });

  let libraryRefreshing = $state(false), libraryLoading = $state(false), libraryError = $state(''), libraryNotice = $state('');
  let jobSearchBusy = $state(false), jobSearchError = $state('');
  let listRevision = 0, searchRevision = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  async function refreshJobs() {
    const revision = ++listRevision;
    libraryLoading = true; libraryError = '';
    try {
      const result = await invoke<JobMeta[]>('list_jobs');
      if(revision === listRevision) { allJobs = result; recentJobs = result.slice(0,6); }
    } catch(e) {
      if(revision === listRevision) libraryError = `Biblioteket kunde inte läsas. ${String(e)}`;
    } finally { if(revision === listRevision) libraryLoading = false; }
  }

  function scheduleJobSearch() {
    clearTimeout(searchTimer);
    ++searchRevision; // Invalidate already-running searches immediately, including their status.
    jobSearchBusy = true; jobSearchError = ''; libraryNotice = '';
    searchTimer = setTimeout(() => void searchJobs(), 250);
  }

  async function searchJobs() {
    clearTimeout(searchTimer);
    if(libraryRefreshing) return; // The refresh below runs the latest query when ready.
    const revision = ++searchRevision;
    const query = jobSearch;
    jobSearchBusy = true; jobSearchError = '';
    try {
      const result = await invoke<JobMeta[]>('search_jobs', {query});
      if(revision === searchRevision && query === jobSearch) historyJobs = result;
    } catch(e) {
      if(revision === searchRevision && query === jobSearch) jobSearchError = `Sökningen kunde inte slutföras. ${String(e)}`;
    } finally { if(revision === searchRevision) jobSearchBusy = false; }
  }

  async function refreshLibrary() {
    if(libraryRefreshing) return;
    libraryRefreshing = true; libraryNotice = ''; libraryError = ''; jobSearchError = '';
    ++searchRevision; clearTimeout(searchTimer); selectedJobIds = [];
    try {
      await invoke<number>('refresh_library');
      await refreshJobs();
      if(!libraryError) libraryNotice = 'Biblioteket är uppdaterat. Dina projekt och versioner är kvar.';
    } catch(e) { libraryError = `Biblioteket kunde inte uppdateras. ${String(e)}`; }
    finally { libraryRefreshing = false; await searchJobs(); }
  }

  /** Refresh the home recent strip and (when open) the History list after a job mutation. */
  async function reloadJobs() {
    await refreshJobs();
    void loadAllActions(); // job/folder changes (delete, rename, move) ripple into the åtaganden overview
    if (screen === "history") await searchJobs();
    // Empty-folder placeholders that now contain a job are redundant; drop them.
    const real = new Set(
      allJobs.flatMap((j) => catSegments(j.category).map((_, i, a) => a.slice(0, i + 1).join("/"))),
    );
    pendingFolders = pendingFolders.filter((p) => !real.has(p));
  }

  // ---- Folder tree operations (History) ----
  function parentOf(path: string): string {
    const i = path.lastIndexOf("/");
    return i < 0 ? "" : path.slice(0, i);
  }

  function toggleExpand(path: string) {
    expandedFolders = expandedFolders.includes(path)
      ? expandedFolders.filter((p) => p !== path)
      : [...expandedFolders, path];
  }

  /** Rewrite a prefix across the empty-folder placeholders (mirrors the backend job rewrite). */
  function rewritePending(from: string, to: string) {
    pendingFolders = pendingFolders
      .map((p) =>
        p === from ? to : p.startsWith(from + "/") ? (to ? to + p.slice(from.length) : p.slice(from.length + 1)) : p,
      )
      .filter(Boolean);
  }

  async function moveJobToFolder(id: string, path: string) {
    const j = historyJobs.find((x) => x.id === id) ?? allJobs.find((x) => x.id === id);
    if (j) await setJobCategory(j, path);
  }

  function createSubfolder(parent: string) {
    const exists = (p: string) => allFolders.some((f) => f.path === p);
    let path = parent ? parent + "/Ny mapp" : "Ny mapp";
    let n = 2;
    while (exists(path)) path = (parent ? parent + "/" : "") + "Ny mapp " + n++;
    pendingFolders = [...pendingFolders, path];
    if (parent && !expandedFolders.includes(parent)) expandedFolders = [...expandedFolders, parent];
    renamingFolder = path;
    folderRenameText = path.split("/").pop() ?? path;
    ctxMenu = null;
  }

  async function commitFolderRename(oldPath: string) {
    const name = folderRenameText.trim();
    renamingFolder = null;
    if (!name) return;
    const newPath = parentOf(oldPath) ? parentOf(oldPath) + "/" + name : name;
    if (newPath === oldPath) return;
    rewritePending(oldPath, newPath);
    try {
      await invoke("move_folder", { from: oldPath, to: newPath });
      if (selectedFolder === oldPath || selectedFolder.startsWith(oldPath + "/"))
        selectedFolder = newPath + selectedFolder.slice(oldPath.length);
      if (currentCategory === oldPath || currentCategory.startsWith(oldPath + "/"))
        currentCategory = newPath + currentCategory.slice(oldPath.length);
      await reloadJobs();
    } catch (e) {
      error = String(e);
    }
  }

  async function deleteFolder(path: string) {
    ctxMenu = null;
    const parent = parentOf(path);
    const count = folderCounts.get(path) ?? 0;
    if (
      count &&
      !(await ask(`Ta bort mappen ”${path}”? ${count} projekt flyttas till ${parent || "Alla"}.`, {
        title: "Ta bort mapp",
        kind: "warning",
      }))
    )
      return;
    rewritePending(path, parent);
    try {
      await invoke("move_folder", { from: path, to: parent });
      if (selectedFolder === path || selectedFolder.startsWith(path + "/")) selectedFolder = parent;
      await reloadJobs();
      showToast("Mappen borttagen");
    } catch (e) {
      error = String(e);
    }
  }

  function openCtx(e: MouseEvent, kind: "folder" | "job", target: string) {
    e.preventDefault();
    e.stopPropagation();
    // Clamp to the viewport so the menu never opens off-screen (rows sit near the right edge).
    const w = 200;
    const h = kind === "folder" ? 140 : 220;
    const x = Math.max(8, Math.min(e.clientX, window.innerWidth - w - 8));
    const y = Math.max(8, Math.min(e.clientY, window.innerHeight - h - 8));
    ctxMenu = { x, y, kind, target };
  }

  // ---- Pointer-based drag-and-drop (more reliable than HTML5 DnD across the webview) ----
  function startDrag(e: MouseEvent, j: JobMeta) {
    if (e.button !== 0) return;
    e.preventDefault();
    dragging = { id: j.id, label: j.title };
    dragX = e.clientX;
    dragY = e.clientY;
    dropTarget = null;
  }

  function onDragMove(e: MouseEvent) {
    if (!dragging) return;
    dragX = e.clientX;
    dragY = e.clientY;
    const el = document.elementFromPoint(e.clientX, e.clientY);
    const folderEl = el?.closest("[data-folder]");
    dropTarget = folderEl ? folderEl.getAttribute("data-folder") : null;
  }

  async function onDragUp() {
    if (!dragging) return;
    const id = dragging.id;
    const target = dropTarget;
    dragging = null;
    dropTarget = null;
    if (target === null) return;
    if (selectedJobIds.includes(id) && selectedJobIds.length > 1) await bulkMove(target);
    else await moveJobToFolder(id, target);
  }

  // ---- Multi-select + bulk actions ----
  function toggleJobSelect(id: string) {
    selectedJobIds = selectedJobIds.includes(id) ? selectedJobIds.filter((x) => x !== id) : [...selectedJobIds, id];
  }
  function clearSelection() {
    selectedJobIds = [];
  }

  async function bulkMove(path: string) {
    const ids = [...selectedJobIds];
    for (const id of ids) {
      const j = allJobs.find((x) => x.id === id);
      if (j) await invoke("update_job_meta", { id, title: j.title, category: path.trim() });
    }
    lastCategory = path.trim() || lastCategory;
    clearSelection();
    await reloadJobs();
    showToast(`${ids.length} projekt flyttade`);
  }

  async function bulkDeleteAudio() {
    const ids = selectedJobIds.filter((id) => (allJobs.find((x) => x.id === id)?.audioBytes ?? 0) > 0);
    if (!ids.length) return;
    for (const id of ids) await invoke("delete_job_audio", { id });
    clearSelection();
    await reloadJobs();
    showToast(`Ljud borttaget i ${ids.length} projekt – texten finns kvar`);
  }

  async function bulkDelete() {
    const ids = [...selectedJobIds];
    if (!ids.length) return;
    if (!(await ask(`Ta bort ${ids.length} projekt? Texten och Avskrift-skapat ljud raderas (uppladdade originalfiler rörs inte).`, { title: "Ta bort projekt", kind: "warning" })))
      return;
    for (const id of ids) {
      if (currentJobId === id) currentJobId = null;
      await invoke("delete_job", { id });
    }
    clearSelection();
    await reloadJobs();
    showToast(`${ids.length} projekt borttagna`);
  }

  /** Free space: delete audio for every project in a folder (recursively), keeping the text. */
  async function deleteFolderAudio(path: string) {
    ctxMenu = null;
    const under = allJobs.filter((j) => j.audioBytes > 0 && (j.category === path || j.category.startsWith(path + "/")));
    if (!under.length) return;
    if (!(await ask(`Ta bort ljudet i ${under.length} projekt i ”${path}”? Texten behålls.`, { title: "Frigör utrymme", kind: "warning" })))
      return;
    for (const j of under) await invoke("delete_job_audio", { id: j.id });
    await reloadJobs();
    showToast("Ljud borttaget – texten finns kvar");
  }

  /** Distinct categories across all jobs, for the category datalist suggestions. */
  const jobCategories = $derived([...new Set(allJobs.map((j) => j.category).filter(Boolean))].sort());

  /** Parse a category string into clean path segments ("Elever/Kalle/HT22" → [Elever,Kalle,HT22]). */
  function catSegments(cat: string): string[] {
    return cat
      ? cat
          .split("/")
          .map((s) => s.trim())
          .filter(Boolean)
      : [];
  }

  /** Every folder path (from jobs + created-but-empty), incl. intermediate ancestors. */
  const allFolders = $derived.by(() => {
    const set = new Set<string>();
    const add = (cat: string) => {
      const parts = catSegments(cat);
      for (let i = 1; i <= parts.length; i++) set.add(parts.slice(0, i).join("/"));
    };
    for (const cat of jobCategories) add(cat);
    for (const p of pendingFolders) add(p);
    return [...set]
      .sort((a, b) => a.localeCompare(b, "sv"))
      .map((path) => ({ path, name: path.split("/").pop() ?? path, depth: path.split("/").length - 1 }));
  });

  /** Tree nodes visible given which folders are expanded; each flags whether it has children. */
  const visibleTree = $derived(
    allFolders
      .filter((f) => {
        const parts = f.path.split("/");
        for (let i = 1; i < parts.length; i++) if (!expandedFolders.includes(parts.slice(0, i).join("/"))) return false;
        return true;
      })
      .map((f) => ({ ...f, hasChildren: allFolders.some((g) => g.path.startsWith(f.path + "/")) })),
  );

  /** Jobs in the right pane: those in the selected folder (recursively); "" = all. */
  const jobsInSelected = $derived(
    selectedFolder
      ? listedJobs.filter((j) => j.category === selectedFolder || j.category.startsWith(selectedFolder + "/"))
      : listedJobs,
  );

  /** Recursive job count per folder path (a job in Elever/Kalle counts for Elever too). */
  const folderCounts = $derived.by(() => {
    const m = new Map<string, number>();
    for (const j of allJobs) {
      const parts = catSegments(j.category);
      for (let i = 1; i <= parts.length; i++) {
        const p = parts.slice(0, i).join("/");
        m.set(p, (m.get(p) ?? 0) + 1);
      }
    }
    return m;
  });

  /** Recursive on-disk audio bytes per folder path, for the storage hints in the tree. */
  const folderBytes = $derived.by(() => {
    const m = new Map<string, number>();
    for (const j of allJobs) {
      const parts = catSegments(j.category);
      for (let i = 1; i <= parts.length; i++) {
        const p = parts.slice(0, i).join("/");
        m.set(p, (m.get(p) ?? 0) + j.audioBytes);
      }
    }
    return m;
  });

  const totalBytes = $derived(allJobs.reduce((s, j) => s + j.audioBytes, 0));

  function fmtBytes(n: number): string {
    if (!n) return "";
    if (n >= 1024 * 1024) return (n / (1024 * 1024)).toFixed(n >= 10 * 1024 * 1024 ? 0 : 1) + " MB";
    return Math.max(1, Math.round(n / 1024)) + " kB";
  }

  function startRename(j: JobMeta) {
    renamingJobId = j.id;
    renameText = j.title;
  }

  async function commitRename(j: JobMeta) {
    const t = renameText.trim();
    renamingJobId = null;
    if (!t || t === j.title) return;
    try {
      await invoke("update_job_meta", { id: j.id, title: t, category: j.category });
      if (currentJobId === j.id) currentJobTitle = t; // keep the active job's stable title in sync
      await reloadJobs();
    } catch (e) {
      error = String(e);
    }
  }

  async function setJobCategory(j: JobMeta, category: string) {
    const cat = category.trim();
    if (cat === j.category) return;
    try {
      await invoke("update_job_meta", { id: j.id, title: j.title, category: cat });
      lastCategory = cat || lastCategory;
      if (currentJobId === j.id) currentCategory = cat;
      await reloadJobs();
      showToast(cat ? `Flyttad till ”${cat}”` : "Mapp rensad");
    } catch (e) {
      error = String(e);
    }
  }

  /** Set the active job's folder from the workspace field; remembers it and persists if saved. */
  async function setCurrentCategory(v: string) {
    const cat = v.trim();
    currentCategory = cat;
    lastCategory = cat || lastCategory;
    const j = allJobs.find((x) => x.id === currentJobId);
    if (currentJobId && j) {
      try {
        await invoke("update_job_meta", { id: currentJobId, title: j.title, category: cat });
        await reloadJobs();
      } catch (e) {
        error = String(e);
      }
    }
  }

  async function deleteJobAudio(j: JobMeta) {
    try {
      await invoke("delete_job_audio", { id: j.id });
      if (currentJobId === j.id) {
        meetingSysWav = null;
        meetingMicWav = null;
        meetingMixWav = null;
        audioPath = null;
      }
      await reloadJobs();
      showToast("Ljudfilen borttagen – texten finns kvar");
    } catch (e) {
      error = String(e);
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
    if (restoringJob) return true;
    const now = new Date().toISOString();
    // Keep an existing work's identity while using its local review/summary tabs.
    type = currentJobId ? currentJobType : type;
    currentJobType = type;
    if (!currentJobId) {
      currentJobId = crypto.randomUUID ? crypto.randomUUID() : String(Date.now());
      currentJobCreatedAt = now;
    }
    // Derive the title once and keep it stable — so re-transcribing or reopening a meeting doesn't
    // rename the project (which made it look like a brand-new/second project).
    if (!currentJobTitle) currentJobTitle = deriveTitle(type);
    if (!originalTranscript?.utterances.length && transcript?.utterances.length) originalTranscript = JSON.parse(JSON.stringify(transcript));
    const job = {
      version: 2,
      agenda, bookmarks, decisions, followupFrom, lastView:view, lastPosition:currentTime,
      meetingWarning, meetingError:currentJobPending?undefined:'',
      originalTranscript,
      originalSourceText,
      summaryBasis,
      summaryAnonymized,
      reviewSnapshot: analysis?.snapshot ?? null,
      reviewIsDocument: deidentDoc,
      groundedWork,
      reviewApprovedBasis,
      selectedProfile,
      id: currentJobId,
      jobType: type,
      title: currentJobTitle,
      createdAt: currentJobCreatedAt ?? now,
      updatedAt: now,
      transcript,
      speakerLabels,
      audioPath: type === "transcribe" || type === "meeting" ? audioPath : null,
      micWavPath: type === "meeting" ? meetingMicWav : null,
      mixWavPath: type === "meeting" ? meetingMixWav : null,
      // Preserve the crash-recovery flag across workspace edits — omitting it would let a note edit on
      // a still-pending meeting silently clear the "ej klar" badge/banner (serde defaults it to false).
      transcriptionPending: currentJobPending,
      category: currentCategory,
      sourceText: srcMode !== "transcript" ? srcText : null,
      sourceMode: srcMode,
      sourceHasTables: srcHasTables,
      sourcePath: type !== "transcribe" && srcMode === "file" ? srcPath : null,
      enabled: ALL_KEYS.filter((k) => enabled[k]),
      terms,
      useAi,
      rejected: [...rejected],
      summaryDraft: summaryDraft || null,
      summaryTemplate: selectedTemplate,
      summaryModel: selectedSummaryModel,
      customHeadings,
      notes,
      participants,
      actions,
      followup,
    };
    const snapshot = JSON.parse(JSON.stringify(job));
    const revision = editRevision;
    saveState = "saving";
    saveError = "";
    try {
      await saveQueue.enqueue(() => invoke("save_job", { job: snapshot }));
      if (currentJobId === snapshot.id && editRevision === revision) {
        saveState = "saved";
        savedAt = now;
        dirty = false;
      }
      void refreshJobs();
      return true;
    } catch (e) {
      if (currentJobId === snapshot.id) {
        saveState = "error";
        saveError = String(e);
      }
      return false;
    }
  }

  // ---- Meeting workspace (B): notes / participants / actions / follow-up ----
  let actionsBusy = $state(false);
  let wsSaveTimer: ReturnType<typeof setTimeout> | null = null;

  /** Live-meeting pane toggles — keep at least one pane visible so the screen never goes blank. */
  function toggleLivePane() {
    meetingShowLive = !meetingShowLive;
    if (!meetingShowLive && !meetingShowNotes) meetingShowNotes = true;
  }
  function toggleNotesPane() {
    meetingShowNotes = !meetingShowNotes;
    if (!meetingShowNotes && !meetingShowLive) meetingShowLive = true;
  }

  /** Clear the workspace so a freshly started job doesn't inherit the previous one's notes/actions. */
  function resetWorkspace() {
    agenda="";bookmarks=[];decisions=[];meetingWarning="";playbackTrack="mix";followupFrom=null;
    summaryAnonymized = false;
    groundedWork = null; reviewApprovedBasis = null;
    notes = "";
    participants = [];
    actions = [];
    followup = "";
    newActionText = "";
    actionPasteOpen = false;
    actionPasteText = "";
    maximized = null;
  }

  /** Persist the workspace fields onto the active job, debounced. Keeps the job's own type so the
   *  badge/title stay correct. No-op until there's something to attach them to. */
  function saveWorkspace() {
    if (restoringJob || (!hasActiveJob && !currentJobId && !srcText.trim())) return;
    if (!currentJobId && !transcript) currentJobType = screen === "summarize" ? "summarize" : "deidentify";
    editRevision++;
    saveState = "pending";
    if (wsSaveTimer) clearTimeout(wsSaveTimer);
    wsSaveTimer = setTimeout(() => {
      wsSaveTimer = null;
      void saveCurrentJob(currentJobType);
    }, 500);
  }

  async function flushCurrentSave(): Promise<boolean> {
    if (dictationPanel && !(await dictationPanel.flushChanges())) { screen = "dictation"; return false; }
    if (editingIdx !== null) await commitEdit();
    if (wsSaveTimer) { clearTimeout(wsSaveTimer); wsSaveTimer = null; }
    await saveQueue.drain();
    if (saveState === "error" || saveState === "pending" || dirty) {
      return await saveCurrentJob(currentJobType);
    }
    return true;
  }

  async function checkpointWork(): Promise<boolean> {
    if (restoringJob) return true;
    if (!(await flushCurrentSave())) return false;
    try {
      if (currentJobId) await invoke("checkpoint_job", {id:currentJobId});
      return true;
    } catch (e) { saveState="error"; saveError=String(e); return false; }
  }

  async function openVersions() {
    if (await flushCurrentSave()) versionsOpen = !!currentJobId;
  }
  async function restoreVersion(version: string) {
    if (busy || qaBusy || actionsBusy || !currentJobId || !(await flushCurrentSave())) return false;
    const id=currentJobId;
    await invoke("restore_job_version", {id,version,now:new Date().toISOString()});
    await openJobById(id);
    return !error;
  }
  function originalText() {
    return [originalSourceText, originalTranscript?.utterances.map(u=>u.text).join("\n")].filter(Boolean).join("\n\n");
  }
  function changeSource(text: string) {
    srcText = text;
    saveWorkspace();
  }
  async function newDocumentFromDictation(text: string) {
    if (busy || qaBusy || actionsBusy || recording || meetingActive || meetingBusy) { error = "Avsluta det pågående arbetet först."; return false; }
    await newProject();
    if (saveState === "error" || currentJobId) return false;
    srcMode="paste"; srcText=text; currentJobType="deidentify";
    saveWorkspace();
    return true;
  }

  onMount(() => {
    const window = getCurrentWindow();
    const registration = window.onCloseRequested(event => {
      if(meetingActive){event.preventDefault();void stopMeeting().then(()=>{if(!meetingActive)void window.close();});return;}
      if(soundTestBusy||meetingBusy){event.preventDefault();return;}
      if (saveState === "pending" || saveState === "saving" || saveState === "error" || dirty || dictationDirty || editingIdx !== null) {
        event.preventDefault();
        void flushCurrentSave().then(ok => { if (ok) void window.close(); });
      }
    });
    return () => { void registration.then(unlisten => unlisten()); if (wsSaveTimer) clearTimeout(wsSaveTimer); };
  });

  function openExport(initial: string) {
    exportChoices = [];
    if (transcript) exportChoices.push({ id: "transcript", label: "Transkript", formats: hasWords ? ["txt", "docx", "srt", "vtt", "vtt-words"] : ["txt", "docx", "srt", "vtt"], status: "Transkript med dina rättningar och talarnamn. Ingen maskering tillämpas.", timestamps: true });
    if (srcText) exportChoices.push({ id: "source", label: "Källtext", formats: ["txt", "docx"], status: "Din aktuella källtext. Ingen maskering tillämpas." });
    if (analysis) exportChoices.push({ id: "anon", label: "Avidentifierad text", formats: deidentDoc ? ["txt", "docx"] : ["txt", "docx", "srt", "vtt"], status: "Valda maskeringar tillämpas. Kontrollera hela texten och eventuella talarnamn före delning.", timestamps: !deidentDoc });
    if (summaryDraft) exportChoices.push({ id: "summary", label: "Sammanfattning / utkast", formats: ["txt", "docx"], status: "AI-utkast med dina redigeringar. Kontrollera innehållet mot källan.", appendTranscript: !!transcript, timestamps: !!transcript });
    if (wsHasContent) exportChoices.push({ id: "notes", label: "Anteckningar och åtgärder", formats: ["txt", "docx"], status: "Deltagare, egna anteckningar, åtgärder och uppföljning." });
    if(currentJobType==='meeting')exportChoices.unshift({id:'meeting',label:'Mötesunderlag',formats:['txt','docx'],sections:[{id:'agenda',label:'Agenda'},{id:'summary',label:'Sammanfattning'},{id:'decisions',label:'Beslut'},{id:'notes',label:'Anteckningar och åtgärder'}],status:'Agenda, sammanfattningsutkast, beslut, egna anteckningar och åtgärder. Granska före delning.',appendTranscript:!!transcript,timestamps:!!transcript});
    if (!exportChoices.length) return;
    exportInitial = exportChoices.some(c => c.id === initial) ? initial : exportChoices[0].id;
    exportOpen = true;
  }

  async function prepareExport(id: string, format: string, timestamps: boolean, append: boolean, sections?:string[]) {
    if (id === "anon" && reviewStale) throw new Error("Underlaget har ändrats. Kör om granskningen före export.");
    const renderTranscript = async (anonymize: boolean) => {
      if (transcript) await invoke("update_transcript", { transcript });
      return await invoke<string>("preview_transcript", { args: { path: "preview." + (format === "vtt-words" ? "vtt" : format), anonymize, rejected: anonymize ? rejectedIds() : [], speakerLabels, wordLevel: format === "vtt-words", timestamps } });
    };
    if(id==='meeting'){
      const include=(key:string)=>!sections||sections.includes(key);
      const parts=[`# ${currentJobTitle}`];
      if(include('agenda')&&agenda)parts.push('## Agenda\n'+agenda);
      if(include('summary')&&summaryDraft)parts.push('## Sammanfattning – utkast\n'+summaryDraft);
      if(include('decisions'))parts.push('## Beslut\n'+decisions.map(d=>'- '+d.text+(d.quote?'\n  Källa: '+d.quote:'')).join('\n'));
      if(include('notes'))parts.push(buildWorkspaceText(),bookmarks.map(b=>`[${fmtTime(b.time)}] ${b.text}`).join('\n'));
      if(append)parts.push('## Transkript\n'+await renderTranscript(false));return parts.filter(Boolean).join('\n\n');
    }
    if (id === "notes") return buildWorkspaceText();
    if (id === "source") return srcText;
    if (id === "summary") return summaryDraft + (append ? "\n\n## Transkript\n\n" + await renderTranscript(false) : "");
    if (id === "anon" && deidentDoc) return await invoke<string>("copy_anonymized", { rejected: rejectedIds() });
    return await renderTranscript(id === "anon");
  }

  async function saveExportSnapshot(text: string, id: string, format: string) {
    const ext = format === "vtt-words" ? "vtt" : format;
    const stem = (currentJobTitle || srcName || audioName || "avskrift").replace(/\.[^.]+$/, "").replace(/[<>:"/\\|?*]/g, "_");
    const suffix: Record<string,string> = { meeting:"motesunderlag", transcript: "transkript", anon: "avidentifierad", summary: "utkast", notes: "anteckningar" };
    const path = await save({ defaultPath: `${stem}_${suffix[id]}.${ext}`, filters: [{ name: ext.toUpperCase(), extensions: [ext] }] });
    if (!path) return false;
    // Save only the displayed snapshot. Never re-read mutable engine state after review.
    await invoke("save_summary", { args: { path, text, includeTranscript: false } });
    return true;
  }

  function addAction() {
    const t = newActionText.trim();
    if (!t) return;
    actions = [...actions, { text: t, done: false, assignee: "", due: "" }];
    newActionText = "";
    saveWorkspace();
  }
  function toggleAction(i: number) {
    actions = actions.map((a, k) => (k === i ? { ...a, done: !a.done } : a));
    saveWorkspace();
  }
  function removeAction(i: number) {
    actions = actions.filter((_, k) => k !== i);
    saveWorkspace();
  }
  function moveAction(i: number, dir: -1 | 1) {
    const j = i + dir;
    if (j < 0 || j >= actions.length) return;
    const next = actions.slice();
    [next[i], next[j]] = [next[j], next[i]];
    actions = next;
    saveWorkspace();
  }

  /** Tolerant parser: one action per line; strips bullets, "[ ]"/"[x]" checkboxes and "1." numbering. */
  function parseActionLines(raw: string): ActionItem[] {
    const out: ActionItem[] = [];
    for (let line of raw.split(/\r?\n/)) {
      line = line.trim();
      if (!line) continue;
      let done = false;
      const cb = line.match(/^[-*•]?\s*\[\s*([xX ])\s*\]\s*(.*)$/);
      if (cb) {
        done = cb[1].toLowerCase() === "x";
        line = cb[2];
      } else {
        line = line.replace(/^[-*•]\s+/, "").replace(/^\d+[.)]\s+/, "");
      }
      line = line.trim();
      if (!line || /^(åtgärder|att göra|to ?do|uppgifter|beslut)\s*:?$/i.test(line)) continue;
      out.push({ text: line, done, assignee: "", due: "" });
    }
    return out;
  }

  function applyActionPaste() {
    const parsed = parseActionLines(actionPasteText);
    if (parsed.length) {
      actions = [...actions, ...parsed];
      saveWorkspace();
      showToast(parsed.length + (parsed.length === 1 ? " åtgärd tillagd" : " åtgärder tillagda"));
    }
    actionPasteText = "";
    actionPasteOpen = false;
  }

  /** Generate an action list locally with Qwen, strictly from the transcript (respects the de-identify
   *  toggle via summaryInputText). Appends whatever it finds; non-locking like the Q&A. */
  async function generateActions() {
    if (actionsBusy || busy || qaBusy || !transcript) return;
    actionsBusy = true;
    error = "";
    progressMsg = "Tar fram åtgärder…";
    try {
      const text = await summaryInputText();
      const q =
        "Lista alla åtgärder, beslut och att-göra-punkter från mötet. Svara med EN punkt per rad och " +
        'börja varje rad med "- ". Skriv ingenting annat än listan. Finns inga åtgärder, svara exakt [INGA_ÅTGÄRDER].';
      const a = await invokeWork<string>("ask_transcript", {
        args: { question: q, transcriptText: text, model: selectedSummaryModel },
      });
      const normalized = a.trim().replace(/^[-*•]\s*/, '');
      const noActions = /^(\[INGA_ÅTGÄRDER\]|\[INGEN_UPPGIFT\]|Det framgår inte av underlaget\.?|Inga åtgärder hittades\.?)$/i.test(normalized);
      const parsed = noActions ? [] : parseActionLines(a);
      if (parsed.length) {
        actions = [...actions, ...parsed];
        saveWorkspace();
        showToast(parsed.length + (parsed.length === 1 ? " åtgärd tillagd" : " åtgärder tillagda"));
      } else {
        showToast("Inga åtgärder hittades");
      }
    } catch (e) {
      error = String(e);
    } finally {
      actionsBusy = false;
      progressMsg = "";
    }
  }

  /** Plain-text dump of the workspace for pasting into an email / journal note. */
  function buildWorkspaceText(): string {
    const parts: string[] = [];
    const ps = participants.filter((p) => p.name.trim() || p.role.trim());
    if (ps.length) {
      parts.push("Deltagare:");
      for (const p of ps) parts.push("- " + p.name.trim() + (p.role.trim() ? " (" + p.role.trim() + ")" : ""));
      parts.push("");
    }
    if (notes.trim()) {
      parts.push("Anteckningar:", notes.trim(), "");
    }
    if (actions.length) {
      parts.push("Att göra:");
      for (const a of actions) {
        const extra = [a.assignee.trim(), a.due.trim()].filter(Boolean).join(", ");
        parts.push("- [" + (a.done ? "x" : " ") + "] " + a.text.trim() + (extra ? " (" + extra + ")" : ""));
      }
      parts.push("");
    }
    if (followup.trim()) parts.push("Uppföljning: " + followup.trim());
    return parts.join("\n").trim();
  }
  async function copyWorkspace() {
    const t = buildWorkspaceText();
    if (!t) return;
    try {
      await navigator.clipboard.writeText(t);
      showToast("Kopierat");
    } catch (e) {
      error = String(e);
    }
  }

  function addParticipant() {
    participants = [...participants, { name: "", role: "" }];
    saveWorkspace();
  }
  function removeParticipant(i: number) {
    participants = participants.filter((_, k) => k !== i);
    saveWorkspace();
  }
  /** Seed participants from the transcript's (renamed) speakers, skipping names already present. */
  function seedParticipantsFromSpeakers() {
    const have = new Set(participants.map((p) => p.name.trim().toLowerCase()).filter(Boolean));
    const add: { name: string; role: string }[] = [];
    for (const id of speakerOptions) {
      const name = (speakerLabels[id] ?? id).trim();
      if (!name || have.has(name.toLowerCase())) continue;
      have.add(name.toLowerCase());
      add.push({ name, role: "" });
    }
    if (add.length) {
      participants = [...participants, ...add];
      saveWorkspace();
      showToast(add.length + (add.length === 1 ? " deltagare tillagd" : " deltagare tillagda"));
    } else {
      showToast("Inga nya talare att lägga till");
    }
  }

  async function openJobById(id: string) {
    if (busy || qaBusy || actionsBusy || recording || meetingActive || meetingBusy) {
      error = "Avsluta det pågående arbetet innan du öppnar ett annat projekt.";
      return;
    }
    if (!(await flushCurrentSave())) return;
    restoringJob = true;
    try {
      const j = await invoke<any>("open_job", { id });
      // Reset working state, then hydrate from the job.
      audioEl?.pause();
      playing = false;
      currentTime = 0;
      audioPath = null; // clear first so the player can't briefly point at the previous project's audio
      meetingSysWav = null;
      meetingMicWav = null;
      meetingMixWav = null;
      transcript = null;
      analysis = null;
      summaryDraft = "";
      deidentDoc = false;
      void invoke('organize_job',{id:j.id,lastOpened:new Date().toISOString()}).then(()=>refreshJobs()).catch(()=>{});
      currentJobId = j.id;
      originalTranscript = JSON.parse(JSON.stringify(j.originalTranscript ?? j.transcript ?? null));
      undoStack = []; editingIdx = null; rejected = new Set();
      originalSourceText = j.originalSourceText ?? (j.version < 2 ? j.sourceText : null) ?? null;
      groundedWork = j.groundedWork ?? null;
      reviewApprovedBasis = j.reviewApprovedBasis ?? null;
      selectedProfile = j.selectedProfile ?? 'skola';
      summaryBasis = j.summaryBasis ?? null;
      summaryAnonymized = j.summaryAnonymized === true;
      saveState = "saved";
      saveError = "";
      savedAt = j.updatedAt ?? "";
      dirty = false;
      currentJobType = j.jobType ?? "transcribe";
      currentJobTitle = j.title ?? "";
      currentJobCreatedAt = j.createdAt ?? null;
      currentJobPending = !!j.transcriptionPending;
      currentCategory = j.category ?? "";
      speakerLabels = j.speakerLabels ?? {};
      agenda=j.agenda??"";bookmarks=j.bookmarks??[];decisions=j.decisions??[];followupFrom=j.followupFrom??null;meetingWarning=j.meetingWarning??j.meetingError??"";playbackTrack="mix";
      notes = typeof j.notes === "string" ? j.notes : "";
      participants = Array.isArray(j.participants) ? j.participants : [];
      actions = Array.isArray(j.actions) ? j.actions : [];
      followup = typeof j.followup === "string" ? j.followup : "";
      maximized = null;
      newActionText = "";
      actionPasteOpen = false;
      actionPasteText = "";
      if (Array.isArray(j.terms)) terms = j.terms;
      if (typeof j.useAi === "boolean") useAi = j.useAi;
      if (j.summaryTemplate) selectedTemplate = j.summaryTemplate;
      if (j.summaryModel) selectedSummaryModel = j.summaryModel;
      if (j.customHeadings) customHeadings = j.customHeadings;
      srcText = j.sourceText ?? "";
      transcript = j.transcript ?? null;
      srcHasTables = !!j.sourceHasTables;
      srcMode = j.sourceMode ?? (j.sourcePath ? "file" : j.sourceText ? "paste" : transcript ? "transcript" : "paste");
      srcPath = j.sourcePath ?? null;
      srcName = j.sourcePath ? j.sourcePath.split(/[\\/]/).pop() : null;
      summaryDraft = j.summaryDraft ?? "";
      if (Array.isArray(j.enabled)) enabled = Object.fromEntries(ALL_KEYS.map(key => [key, j.enabled.includes(key)]));

      if (j.jobType === "transcribe" || j.jobType === "meeting") {
        transcript = j.transcript ?? null;
        audioPath = j.audioPath ?? null;
        audioName = audioPath ? audioPath.split(/[\\/]/).pop() ?? audioPath : null;
        meetingSysWav = j.jobType === "meeting" ? j.audioPath ?? null : null;
        meetingMicWav = j.jobType === "meeting" ? j.micWavPath ?? null : null;
        meetingMixWav = j.jobType === "meeting" ? j.mixWavPath ?? null : null;
        summaryDraft = j.summaryDraft ?? "";
        view = ["overview","transcript","notes","actions","summary","review","qa"].includes(j.lastView)?j.lastView:(j.jobType==="meeting"?"overview":j.summaryDraft?"summary":"transcript");
        currentTime=j.lastPosition??0;
        screen = "transcribe";
      } else if (j.jobType === "deidentify") {
        screen = "deidentify";
        // Legacy jobs open their saved source without silently running a model or replacing the file.
      } else {
        transcript = j.transcript ?? null;
        summaryDraft = j.summaryDraft ?? "";
        view = "summary";
        screen = "summarize";
      }
      if (j.reviewSnapshot) {
        deidentDoc = !!j.reviewIsDocument;
        // A stale review stays attached to its own source and is marked stale in the workspace.
        analysis = await invoke<AnalyzeResult>("restore_review", { snapshot:j.reviewSnapshot, expectedText:j.reviewSnapshot.text });
        rejected = new Set(j.rejected ?? []);
        if (j.jobType === "deidentify") view = "review";
      }
      // Keep the backend transcript in sync with what's shown, so export uses the right one
      // (export reads backend.transcript; de-identify already uses the frontend transcript).
      if (transcript) await invoke("update_transcript", { transcript }).catch(() => {});
      // Force the player to load THIS project's audio (a changed src alone doesn't always reload).
      await tick();
      audioEl?.load();
      error = "";
      showToast("Jobb öppnat");
    } catch (e) {
      error = String(e);
    } finally {
      restoringJob = false;
    }
  }

  async function deleteJobById(id: string) {
    if (!(await ask("Ta bort det här jobbet ur historiken?", { title: "Ta bort jobb", kind: "warning" }))) return;
    try {
      await invoke("delete_job", { id });
      if (currentJobId === id) currentJobId = null;
      await reloadJobs();
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

<svelte:window onmousemove={onDragMove} onmouseup={onDragUp} />
{#if dragging}
  <div class="drag-ghost" style="left:{dragX}px; top:{dragY}px">{dragging.label}</div>
{/if}

<div class="app" class:grabbing={!!dragging}>
  {#snippet folderPicker(current: string, onPick: (p: string) => void)}
    <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
    <div class="fp-backdrop" onclick={() => (folderPickerFor = null)} role="presentation"></div>
    <div class="fp-menu">
      <button class="fp-item" class:on={!current} onclick={() => onPick("")}>Ingen mapp</button>
      {#each allFolders as f (f.path)}
        <div class="fp-row" style="padding-left:{f.depth * 15}px">
          <button class="fp-item" class:on={current === f.path} onclick={() => onPick(f.path)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2h7A1.5 1.5 0 0 1 19 9.5v7A1.5 1.5 0 0 1 17.5 18h-13A1.5 1.5 0 0 1 3 16.5z"/></svg>
            <span class="fp-name">{f.name}</span>
            <span class="fp-count">{folderCounts.get(f.path) ?? 0}</span>
          </button>
          <button class="fp-sub" title={"Skapa undermapp i " + f.name} onclick={() => (newFolderName = f.path + "/")}>+</button>
        </div>
      {/each}
      <div class="fp-create">
        <input
          bind:value={newFolderName}
          placeholder="Ny mapp… ( / för undermapp)"
          onkeydown={(e) => { if (e.key === "Enter" && newFolderName.trim()) onPick(newFolderName.trim()); }}
        />
        <button class="btn small" disabled={!newFolderName.trim()} onclick={() => onPick(newFolderName.trim())}>Skapa</button>
      </div>
    </div>
  {/snippet}
  <AppNavigation active={screen === "transcribe" ? "meeting" : screen} meetingActive={meetingActive || meetingBusy || bgMeetings.length > 0} dictationActive={!!dictation && dictation.phase !== "idle"} overdue={taskCounts.overdue} version={appVersion}
    onmodels={openModels} onnavigate={(page) => go(page as Screen)} onnew={() => void newProject()} />
  <div class="app-content">
  <header class="workspace-header">
    <div class="workspace-heading"><span class="workspace-location">{screen === "home" ? "Ditt arbete" : screen === "history" ? "Bibliotek" : screen === "dictation" ? "Diktering" : screen === "deidentify" ? "Avidentifiering" : screen === "summarize" ? "Sammanfatta text" : screen === "tasks" ? "Åtaganden" : "Möten och transkribering"}</span>
      {#if currentJobTitle && !["home", "history", "tasks", "dictation"].includes(screen)}<strong>{currentJobTitle}</strong><button class="link" onclick={()=>editTitle()}>Byt namn</button>{/if}
    </div>
    <div class="spacer"></div>
    {#if bgMeetings.length}<div class="working" role="status"><span class="working-dot"></span>{bgMeetings.length} {bgMeetings.length === 1 ? "möte bearbetas" : "möten bearbetas"}</div>{/if}
    {#if saveState !== "idle"}<div class="save-status" class:failed={saveState === "error"} role="status">{saveState === "saving" ? "Sparar på datorn…" : saveState === "pending" ? "Ändringar väntar på att sparas" : saveState === "error" ? "Kunde inte spara" : "Sparat på datorn"}{#if saveState === "saved" && savedAt}<span>{new Date(savedAt).toLocaleTimeString("sv-SE", {hour:"2-digit", minute:"2-digit"})}</span>{/if}</div>{/if}
    {#if currentJobId && !["home","history","tasks","dictation"].includes(screen)}<button class="btn" onclick={openVersions} disabled={busy || qaBusy || actionsBusy || meetingActive || meetingBusy}>Original och versioner</button>{/if}
    {#if currentJobId && !["home","history","tasks","dictation"].includes(screen)}{@const meta=allJobs.find(j=>j.id===currentJobId)}{#if meta}{@render workMenu(meta)}{/if}{/if}
    {#if hasActiveJob && !["home","history","tasks","dictation"].includes(screen)}
      <div class="hdr-folder">
        <button class="hdr-folder-btn" onclick={() => (folderPickerFor = folderPickerFor === "header" ? null : "header")} title="Mapp för det här projektet">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2h7A1.5 1.5 0 0 1 19 9.5v7A1.5 1.5 0 0 1 17.5 18h-13A1.5 1.5 0 0 1 3 16.5z"/></svg>
          <span class="hdr-folder-name">{currentCategory || "Mapp…"}</span>
          <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M6 9l6 6 6-6"/></svg>
        </button>
        {#if folderPickerFor === "header"}{@render folderPicker(currentCategory, (p) => { folderPickerFor = null; newFolderName = ""; void setCurrentCategory(p); })}{/if}
      </div>
    {/if}
  </header>
  {#if titleEditing}<form class="title-editor" onsubmit={(e)=>{e.preventDefault();void saveTitle();}}>
    <label>Namn<input bind:this={titleInput} aria-label="Namn på arbetet" bind:value={titleDraft} onkeydown={e=>{if(e.key==='Escape')titleEditing=false;}} /></label>
    <button class="btn primary" disabled={renameBusy||!titleDraft.trim()}>Spara namn</button><button class="btn" type="button" onclick={()=>titleEditing=false}>Avbryt</button>
  </form>{/if}
  <WorkControl />
  {#if saveState === "error"}
    <div class="save-failure" role="alert"><div><strong>Ändringarna kunde inte sparas på datorn.</strong><p>{saveError}</p></div><button class="btn" onclick={() => flushCurrentSave()}>Försök spara igen</button><button class="btn" onclick={() => openExport("notes")}>Exportera en kopia…</button></div>
  {/if}
  {#if reviewStale && (screen === "deidentify" || (screen === "transcribe" && view === "review"))}
    <div class="banner warn" role="status">Underlaget har ändrats. Granskningen nedan gäller den tidigare textversionen. Kör om avidentifieringen innan du exporterar.</div>
  {/if}
  {#if summaryStale && (screen === "summarize" || (screen === "transcribe" && view === "summary"))}
    <div class="banner warn" role="status">Underlaget har ändrats sedan utkastet skapades. Utkastet finns kvar; granska det eller skapa ett nytt.</div>
  {/if}
  {#if error && ["home", "history", "dictation", "tasks", "meeting"].includes(screen)}<div class="banner" class:error={!isWorkCancelled(error)} class:cancelled={isWorkCancelled(error)} role={isWorkCancelled(error)?"status":"alert"}>{error}</div>{/if}
  {#if screen === "transcribe" && (transcript || currentJobPending)}
    <nav class="workspace-tabs" aria-label="Vyer i aktuellt arbete">
      {#if currentJobType==="meeting"}<button aria-pressed={view==='overview'} onclick={()=>tab('overview')}>Översikt</button>{/if}
      <button aria-pressed={view === "transcript"} onclick={() => tab("transcript")}>Transkript</button>
      <button aria-pressed={(view === "notes" || view === "actions")} onclick={() => tab("notes")}>Anteckningar</button>
      <button aria-pressed={view==='actions'} onclick={()=>tab('actions')}>Beslut och åtgärder</button>
      <button aria-pressed={view === "review"} onclick={() => tab("review")}>Avidentifiering</button>
      <button aria-pressed={view === "summary"} onclick={() => tab("summary")}>Sammanfattning</button>
      <button aria-pressed={view === "qa"} onclick={() => tab("qa")}>Fråga källan</button>
    </nav>
  {/if}
  {#if (screen === "transcribe" && transcript && !["overview","notes","actions"].includes(view)) || screen === "deidentify" || screen === "summarize"}
    <div class="panel-control"><button class="link" aria-expanded={!controlsCollapsed} onclick={() => {if(transcriptReading)transcriptToolsOpen=!transcriptToolsOpen;else sidebarCollapsed=!sidebarCollapsed;}}>{transcriptReading ? (transcriptToolsOpen ? "Dölj verktyg för transkriptet" : "Visa verktyg för transkriptet") : (sidebarCollapsed ? "Visa källa och inställningar" : "Dölj källa och inställningar")}</button></div>
  {/if}

  {#snippet workMenu(j:JobMeta)}
    <details class="work-menu"><summary aria-label={'Hantera '+j.title}>Hantera</summary><div>
      <button onclick={()=>editTitle(j)}>Byt namn</button><button onclick={async()=>{await openJobById(j.id);folderPickerFor='header';}}>Flytta</button>
      <button onclick={()=>organize(j,'pinned')}>{j.pinned?'Lossa':'Fäst'}</button><button onclick={()=>organize(j,'archived')}>{j.archived?'Återställ från arkivet':'Arkivera'}</button>
    </div></details>
  {/snippet}
  {#snippet savedDictations()}
    {#if dictation?.entries.some(entry=>entry.saved)}
      <section class="saved-dictations"><h2>Sparade diktat</h2><ul class="job-strip">
        {#each dictation.entries.filter(entry=>entry.saved && (screen !== "history" || !jobSearch || entry.text.toLocaleLowerCase("sv").includes(jobSearch.toLocaleLowerCase("sv")))) as entry (entry.id)}
          <li><button class="job-row" onclick={()=>{go("dictation"); void dictationPanel.openEntry(entry.id);}}><span class="job-badge">Diktat</span><span class="job-title">{entry.text.slice(0,100)}</span><span class="job-date">{new Date(entry.createdAt).toLocaleDateString("sv-SE")}</span></button></li>
        {/each}
      </ul></section>
    {/if}
  {/snippet}
  {#snippet sourcePicker()}
    <section>
      <h2>Källa</h2>
      <div class="src-modes">
        <label class="radio"><input type="radio" name="src" value="paste" bind:group={srcMode} /> Klistra in text</label>
        <label class="radio"><input type="radio" name="src" value="file" bind:group={srcMode} /> Dokument (.txt/.docx)</label>
        {#if transcript}<label class="radio"><input type="radio" name="src" value="transcript" bind:group={srcMode} /> Från transkriptet</label>{/if}
      </div>
      {#if srcMode === "paste"}
        <textarea class="src-text" value={srcText} oninput={(e)=>changeSource(e.currentTarget.value)} aria-label="Källtext" rows="8" placeholder="Klistra in texten här…"></textarea>
      {:else if srcMode === "file"}
        {#if srcName}
          <div class="file-chip"><span title={srcPath}>{srcName}</span><button class="link" onclick={clearSource}>rensa</button></div>
        {:else}
          <button class="btn block" onclick={pickSourceDoc}>Välj dokument…</button>
          <p class="hint">.txt, .md eller .docx. Brödtext och tabellceller läses in; sidhuvuden, sidfötter, bilder och textfält behöver kontrolleras i originalet.</p>
        {/if}
      {:else}
        <p class="hint">Använder transkriptet som redan finns i appen.</p>
      {/if}
    </section>
  {/snippet}

  {#snippet modelReference(kind:'speech'|'text')}
    {@const model = kind==='speech' ? models.find(m=>m.id===selectedModel) : summaryModels.find(m=>m.id===selectedSummaryModel)}
    <div class="model-reference"><span>{kind==='speech'?'Talmodell':'Textmodell'}: <strong>{model?.label ?? (kind==='speech'?selectedModel:selectedSummaryModel)}</strong></span>
      <button class="link" onclick={openModels}>{model?.downloaded?'Byt modell':'Välj eller hämta modell'}</button>
      {#if !model?.downloaded}<small>Behöver hämtas före bearbetning.</small>{/if}
    </div>
  {/snippet}
  {#if modelsOpen}
    <ModelSettings {models} textModels={summaryModels} bind:speech={selectedModel} bind:text={selectedSummaryModel} dictationModel={dictation?.settings.model}
      locked={busy || recording || recSaving || meetingActive || meetingBusy || bgMeetings.length>0 || qaBusy || actionsBusy || !!dictation && dictation.phase!=='idle'} {downloading} percent={downloadPct} {error}
      ondownload={(kind,id)=>kind==='speech'?downloadModel(id):downloadSummaryModel(id)} ondictation={setDictationModel} onchange={()=>{if(currentJobId)saveWorkspace();}} onclose={()=>modelsOpen=false} />
  {/if}

  <div class="dictation-container" hidden={screen !== "dictation"}>
    <Dictation bind:this={dictationPanel} bind:dirty={dictationDirty} snapshot={dictation} {models} textModel={selectedSummaryModel} textReady={summaryDownloaded} onmodels={openModels}
      ontext={async(text) => { if(await newDocumentFromDictation(text)) go("deidentify"); }} />
  </div>
  {#if screen === "home"}
    <div class="home">
      <h2 class="big-title">Ditt arbete</h2>
      <p class="home-intro">Fortsätt där du slutade, eller börja något nytt.</p>
      {#if meetingActive||bgMeetings.length}<section class="home-current"><h2>Pågår nu</h2>{#if meetingActive}<button class="btn" onclick={()=>go('meeting')}>Till inspelningen: {currentJobTitle}</button>{/if}{#each bgMeetings as m}<button class="btn" onclick={()=>openJobById(m.id)}>{m.title} — {m.msg}</button>{/each}</section>{/if}
      <div class="entrypoints">
        <button onclick={() => go("meeting")}><h3>Möten</h3><p>Från samtal och ljudfiler till anteckningar och beslut.</p><span>Öppna möten</span></button>
        <button onclick={() => go("dictation")}><h3>Diktering</h3><p>Tala där du skriver. Hitta texten igen när du behöver den.</p><span>Öppna diktering</span></button>
        <button onclick={() => go("deidentify")}><h3>Avidentifiering</h3><p>Granska uppgifter i text och dokument och skapa en maskerad kopia.</p><span>Öppna avidentifiering</span></button>
      </div>
      <div class="home-tools"><button class="link" onclick={() => go("transcribe")}>Transkribera en ljudfil</button><button class="link" onclick={() => go("summarize")}>Sammanfatta en text</button><button class="link" onclick={() => go("history")}>Öppna biblioteket</button></div>
      {#if recentJobs.length}
        <div class="recent">
          <h2>Fortsätt arbeta</h2>
          <ul class="job-strip">
            {#each continuedJobs as j (j.id)}
              <li>
                <button class="job-row" onclick={() => openJobById(j.id)}>
                  <span class="job-badge {j.jobType}">{JOB_LABELS[j.jobType] ?? j.jobType}</span>
                  <span class="job-title">{j.pinned?"★ ":""}{j.title}<small class="job-path">{j.category}{j.actionsTotal?` · ${j.actionsTotal-j.actionsDone} öppna åtgärder`:""}</small></span>
                  <span class="job-date">{fmtJobDate(j.updatedAt)}</span>
                </button>
                {@render workMenu(j)}
              </li>
            {/each}
          </ul>
        </div>
      {/if}

      <section class="home-followups"><h2>Att följa upp</h2>
        {#each allActions.filter(a=>!a.done&&a.due).sort((a,b)=>a.due.localeCompare(b.due)).slice(0,5) as a}<button class="followup-row" onclick={()=>{if(a.jobId)void openJobById(a.jobId);else go('tasks');}}><span>{a.text}</span><span>{a.assignee} · {a.due}</span></button>{/each}
        {#each followupJobs as j}<button class="followup-row" onclick={()=>openJobById(j.id)}><span>{j.title}</span><span>Uppföljning {j.followup}</span></button>{/each}
        <button class="link" onclick={()=>go('tasks')}>Visa alla åtaganden</button>
      </section>
      {@render savedDictations()}
      <button class="link home-open" onclick={openProject}>Öppna sparat projekt (.avskrift)…</button>
    </div>

  {:else if screen === "history"}
    {#snippet jobRow(j: JobMeta, showPath: boolean)}
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <li class="job-item" class:dragging={dragging?.id === j.id} class:sel={selectedJobIds.includes(j.id)} oncontextmenu={(e) => openCtx(e, "job", j.id)}>
        <input class="job-check" type="checkbox" checked={selectedJobIds.includes(j.id)} onchange={() => toggleJobSelect(j.id)} aria-label="Markera projekt" />
        <span class="drag-handle" title="Dra för att flytta till en mapp" onmousedown={(e) => startDrag(e, j)}>⠿</span>
        {#if renamingJobId === j.id}
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="job-rename"
            bind:value={renameText}
            autofocus
            onblur={() => commitRename(j)}
            onkeydown={(e) => {
              if (e.key === "Enter") commitRename(j);
              if (e.key === "Escape") renamingJobId = null;
            }}
          />
        {:else}
          <button class="job-row" onclick={() => openJobById(j.id)}>
            <span class="job-badge {j.jobType}">{JOB_LABELS[j.jobType] ?? j.jobType}</span>
            <span class="job-title">{j.title}{#if showPath && j.category}<span class="job-path"> · {j.category}</span>{/if}</span>
            {#if j.transcriptionPending}<span class="job-chip pending" title="Transkriberingen är inte klar – öppna och välj Transkribera om">⏳ ej klar</span>{/if}
            {#if j.actionsTotal || j.hasNotes}
              <span class="job-ws">
                {#if j.hasNotes}<span class="job-chip" title="Har anteckningar"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M5 4h10l4 4v12H5z"/><path d="M9 11h6M9 15h4"/></svg></span>{/if}
                {#if j.actionsTotal}<span class="job-chip" class:done={j.actionsDone === j.actionsTotal} title="Åtgärder klara"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8"><path d="M9 11l3 3 8-8"/><path d="M20 12v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h9"/></svg>{j.actionsDone}/{j.actionsTotal}</span>{/if}
              </span>
            {/if}
            <span class="job-date">{fmtJobDate(j.updatedAt)}{j.audioBytes ? " · " + fmtBytes(j.audioBytes) : ""}</span>
          </button>
        {/if}
        <div class="job-cat-wrap">
          <button class="job-cat-btn" onclick={() => (folderPickerFor = folderPickerFor === j.id ? null : j.id)} title="Flytta till mapp">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6"><path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2h7A1.5 1.5 0 0 1 19 9.5v7A1.5 1.5 0 0 1 17.5 18h-13A1.5 1.5 0 0 1 3 16.5z"/></svg>
            <span>{j.category || "Mapp…"}</span>
          </button>
          {#if folderPickerFor === j.id}{@render folderPicker(j.category, (p) => { folderPickerFor = null; newFolderName = ""; void setJobCategory(j, p); })}{/if}
        </div>
        {@render workMenu(j)}
        <button class="job-menu" onclick={(e) => openCtx(e, "job", j.id)} aria-label="Fler åtgärder" title="Fler åtgärder (eller högerklicka)">⋯</button>
      </li>
    {/snippet}

    <div class="library-tools">
      <label>Visa<select aria-label="Biblioteksfilter" bind:value={libraryFilter}><option value="active">Aktiva arbeten</option><option value="pinned">Fästa</option><option value="archived">Arkiverade</option></select></label>
      <label>Typ<select aria-label="Typ av arbete" bind:value={libraryType}><option value="">Alla typer</option><option value="meeting">Möten</option><option value="transcribe">Ljudfiler</option><option value="deidentify">Avidentifiering</option><option value="summarize">Sammanfattning</option></select></label>
      <label class="library-search">Sök bland projekten<input class="job-search" type="search" placeholder="Sök i namn och innehåll…" bind:value={jobSearch} oninput={() => { scheduleJobSearch(); selectedJobIds = []; }} /></label>
      <button class="btn" onclick={refreshLibrary} disabled={libraryRefreshing}>{libraryRefreshing?'Uppdaterar biblioteket…':'Uppdatera biblioteket'}</button>
    </div>
    {#if libraryRefreshing}<p class="library-status" role="status">Läser sparade projekt. Första uppdateringen kan ta en stund i ett stort bibliotek.</p>
    {:else if jobSearchBusy || libraryLoading}<p class="library-status" role="status">{jobSearch.trim()?'Söker bland projekten…':'Läser projekt…'}</p>
    {:else if libraryNotice}<p class="library-status" role="status">{libraryNotice}</p>{/if}
    {#if libraryError}<p class="library-status" role="alert">{libraryError}</p>{/if}
    {#if jobSearchError}<div class="library-status" role="alert">{jobSearchError} <button class="link" onclick={searchJobs}>Försök söka igen</button></div>{/if}
    <div class="hist">
      {#if !allJobs.length && !dictation?.entries.some(entry=>entry.saved)}
        {#if !libraryLoading && !libraryRefreshing && !libraryError}<p class="hint big-hint" style="padding:32px">Inga sparade jobb än. Allt du transkriberar, avidentifierar eller sammanfattar sparas automatiskt och dyker upp här.</p>{/if}
      {:else}
        <aside class="hist-tree">
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <button
            class="tree-node root"
            class:on={!selectedFolder && !jobSearch.trim()}
            class:drop={dropTarget === ""}
            data-folder=""
            onclick={() => { selectedFolder = ""; jobSearch = ""; selectedJobIds = []; scheduleJobSearch(); }}
          >
            <span class="tree-twirl-spacer"></span>
            <svg class="tree-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2h7A1.5 1.5 0 0 1 19 9.5v7A1.5 1.5 0 0 1 17.5 18h-13A1.5 1.5 0 0 1 3 16.5z"/></svg>
            <span class="tree-name">Bibliotek</span><span class="tree-count">{allJobs.length}{totalBytes ? " · " + fmtBytes(totalBytes) : ""}</span>
          </button>
          {#each visibleTree as n (n.path)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="tree-rowwrap" class:drop={dropTarget === n.path} data-folder={n.path} style="padding-left:{n.depth * 14}px">
              {#if n.hasChildren}
                <button class="tree-twirl" class:open={expandedFolders.includes(n.path)} onclick={() => toggleExpand(n.path)} aria-label="Fäll ut/ihop">▸</button>
              {:else}
                <span class="tree-twirl-spacer"></span>
              {/if}
              {#if renamingFolder === n.path}
                <!-- svelte-ignore a11y_autofocus -->
                <input
                  class="tree-rename"
                  bind:value={folderRenameText}
                  autofocus
                  onblur={() => commitFolderRename(n.path)}
                  onkeydown={(e) => { if (e.key === "Enter") commitFolderRename(n.path); if (e.key === "Escape") renamingFolder = null; }}
                />
              {:else}
                <button class="tree-node" class:on={selectedFolder === n.path} onclick={() => { selectedFolder = n.path; selectedJobIds = []; }} oncontextmenu={(e) => openCtx(e, "folder", n.path)}>
                  <svg class="tree-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2h7A1.5 1.5 0 0 1 19 9.5v7A1.5 1.5 0 0 1 17.5 18h-13A1.5 1.5 0 0 1 3 16.5z"/></svg>
                  <span class="tree-name">{n.name}</span><span class="tree-count">{folderCounts.get(n.path) ?? 0}{folderBytes.get(n.path) ? " · " + fmtBytes(folderBytes.get(n.path) ?? 0) : ""}</span>
                </button>
              {/if}
            </div>
          {/each}
          <button class="tree-new" onclick={() => createSubfolder("")}>+ Ny mapp</button>
        </aside>

        <main class="hist-main">
          {#if selectedJobIds.length}
            <div class="bulkbar">
              <span class="bulk-n">{selectedJobIds.length} markerade</span>
              <div class="bulk-move">
                <button class="btn small" onclick={() => (folderPickerFor = "bulk")}>Flytta till…</button>
                {#if folderPickerFor === "bulk"}{@render folderPicker("", (p) => { folderPickerFor = null; newFolderName = ""; void bulkMove(p); })}{/if}
              </div>
              <button class="btn small" onclick={bulkDeleteAudio}>Ta bort ljud</button>
              <button class="btn small" onclick={bulkDelete}>Ta bort</button>
              <button class="btn small" onclick={clearSelection}>Avmarkera</button>
            </div>
          {/if}
          {#if jobSearch.trim()}
            {#if !jobSearchBusy && !libraryRefreshing && !jobSearchError}
            {#if !historyJobs.length}<p class="hint">Inga träffar för ”{jobSearch}”.</p>{/if}
            <ul class="job-list">{#each listedJobs as j (j.id)}{@render jobRow(j, true)}{/each}</ul>
            {/if}
          {:else}
            <div class="hist-bar">
              <span class="hist-where">{selectedFolder || "Bibliotek"}</span>
              <span class="hist-count">{jobsInSelected.length} projekt</span>
            </div>
            {#if jobsInSelected.length}
              <ul class="job-list">{#each jobsInSelected as j (j.id)}{@render jobRow(j, !!selectedFolder)}{/each}</ul>
            {:else}
              <p class="hint">Inga projekt här ännu — dra hit ett projekt, eller välj mapp via ”Mapp…”.</p>
            {/if}
          {/if}
          {#if !selectedFolder || jobSearch.trim()}{@render savedDictations()}{/if}
        </main>
      {/if}
    </div>

    {#if ctxMenu}
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div class="fp-backdrop" onclick={() => (ctxMenu = null)} role="presentation"></div>
      <div class="ctx" style="left:{ctxMenu.x}px; top:{ctxMenu.y}px">
        {#if ctxMenu.kind === "folder"}
          {@const path = ctxMenu.target}
          <button class="ctx-item" onclick={() => createSubfolder(path)}>Ny undermapp</button>
          <button class="ctx-item" onclick={() => { renamingFolder = path; folderRenameText = path.split("/").pop() ?? path; ctxMenu = null; }}>Byt namn</button>
          {#if folderBytes.get(path)}<button class="ctx-item" onclick={() => deleteFolderAudio(path)}>Ta bort ljud i mappen ({fmtBytes(folderBytes.get(path) ?? 0)})</button>{/if}
          <button class="ctx-item danger" onclick={() => deleteFolder(path)}>Ta bort mapp</button>
        {:else}
          {@const id = ctxMenu.target}
          <button class="ctx-item" onclick={() => { openJobById(id); ctxMenu = null; }}>Öppna</button>
          <button class="ctx-item" onclick={() => { const j = historyJobs.find((x) => x.id === id); if (j) startRename(j); ctxMenu = null; }}>Byt namn</button>
          <button class="ctx-item" onclick={() => { folderPickerFor = id; ctxMenu = null; }}>Flytta till…</button>
          <button class="ctx-item" onclick={() => { const j = historyJobs.find((x) => x.id === id); ctxMenu = null; if (j && j.audioBytes) void deleteJobAudio(j); }}>Ta bort ljud</button>
          <button class="ctx-item danger" onclick={() => { const i = ctxMenu?.target; ctxMenu = null; if (i) void deleteJobById(i); }}>Ta bort projekt</button>
        {/if}
    </div>
    {/if}

  {:else if screen === "tasks"}
    <div class="tasks">
      <div class="tasks-head">
        <h2 class="big-title">Åtaganden</h2>
        <p class="tasks-sub">
          {taskCounts.open} öppna{#if taskCounts.overdue} · <span class="t-overdue-text">{taskCounts.overdue} förfallna</span>{/if} · {taskCounts.done} klara — samlade från alla projekt
        </p>
      </div>

      <form class="task-add" onsubmit={(e) => { e.preventDefault(); addTask(); }}>
        <input class="ta-text" bind:value={newTaskText} placeholder="Nytt åtagande…" />
        <input class="ta-who" bind:value={newTaskAssignee} placeholder="Ansvarig (valfritt)" />
        <input class="ta-due" type="date" bind:value={newTaskDue} aria-label="Datum (valfritt)" />
        <select class="profile ta-target" bind:value={newTaskTarget} aria-label="Lägg i mapp eller koppla till projekt">
          <option value="standalone">Fristående (ingen mapp)</option>
          {#if addFolders.length}
            <optgroup label="Lägg i mapp">
              {#each addFolders as f (f)}<option value={"folder:" + f}>{f}</option>{/each}
            </optgroup>
          {/if}
          {#if allJobs.length}
            <optgroup label="Koppla till projekt">
              {#each allJobs as j (j.id)}<option value={"job:" + j.id}>{j.title}</option>{/each}
            </optgroup>
          {/if}
        </select>
        <button class="btn primary" type="submit" disabled={!newTaskText.trim() || taskBusy}>Lägg till</button>
      </form>

      <div class="tasks-bar">
        <div class="seg">
          <button class:on={taskStatus === "open"} onclick={() => (taskStatus = "open")}>Öppna</button>
          <button class:on={taskStatus === "done"} onclick={() => (taskStatus = "done")}>Klara</button>
          <button class:on={taskStatus === "all"} onclick={() => (taskStatus = "all")}>Alla</button>
        </div>
        <input class="task-search" type="search" bind:value={taskQuery} placeholder="Sök i åtgärder, ansvarig, projekt…" />
        {#if taskAssignees.length}
          <select class="profile" bind:value={taskAssignee} aria-label="Ansvarig">
            <option value="">Alla ansvariga</option>
            {#each taskAssignees as who (who)}<option value={who}>{who}</option>{/each}
          </select>
        {/if}
        {#if taskFolders.length}
          <select class="profile" bind:value={taskFolder} aria-label="Mapp">
            <option value="">Alla mappar</option>
            {#each taskFolders as f (f)}<option value={f}>{f}</option>{/each}
          </select>
        {/if}
        <select class="profile" bind:value={taskDue} aria-label="Datum">
          <option value="all">Alla datum</option>
          <option value="overdue">Förfallna</option>
          <option value="week">Denna vecka</option>
          <option value="none">Utan datum</option>
        </select>
        <select class="profile" bind:value={taskSort} aria-label="Sortering">
          <option value="due">Sortera: datum</option>
          <option value="updated">Sortera: senast ändrad</option>
          <option value="project">Sortera: projekt</option>
        </select>
        <select class="profile" bind:value={taskGroup} aria-label="Gruppering">
          <option value="flat">Platt lista</option>
          <option value="project">Gruppera: projekt</option>
          <option value="assignee">Gruppera: ansvarig</option>
        </select>
      </div>

      {#if !filteredActions.length}
        <p class="hint big-hint" style="padding:26px 2px">
          {allActions.length
            ? "Inga åtaganden matchar filtret."
            : "Inga åtaganden än. Skapa ett ovan, eller lägg till åtgärder inne i ett möte eller projekt."}
        </p>
      {:else}
        {#each groupedActions as g (g.key)}
          {#if taskGroup !== "flat"}<h3 class="task-group">{g.key}<span class="tg-count">{g.rows.length}</span></h3>{/if}
          <ul class="task-list">
            {#each g.rows as a (rowKey(a))}
              <li class="task-row" class:done={a.done} class:overdue={dueState(a.due) === "overdue" && !a.done}>
                <input
                  class="t-check"
                  type="checkbox"
                  checked={a.done}
                  onchange={(e) => updateRow(a, { done: e.currentTarget.checked })}
                  aria-label="Markera som klar"
                />
                <div class="t-main">
                  <input class="t-text" value={a.text} onchange={(e) => updateRow(a, { text: e.currentTarget.value })} placeholder="Åtgärd…" />
                  <div class="t-meta">
                    {#if a.source === "standalone"}
                      {#if a.category}
                        <span class="t-proj free" title={"Mapp: " + a.category}>
                          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M3 7.5A1.5 1.5 0 0 1 4.5 6h4l2 2h7A1.5 1.5 0 0 1 19 9.5v7A1.5 1.5 0 0 1 17.5 18h-13A1.5 1.5 0 0 1 3 16.5z"/></svg>
                          {a.category.split("/").pop()}
                        </span>
                      {:else}
                        <span class="t-proj free">Fristående</span>
                      {/if}
                    {:else}
                      <button class="t-proj" onclick={() => openRowProject(a)} title="Öppna projektet">
                        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M5 4h10l4 4v12H5z"/><path d="M9 11h6M9 15h4"/></svg>
                        {projName(a)}
                      </button>
                    {/if}
                    <input class="t-who" value={a.assignee} onchange={(e) => updateRow(a, { assignee: e.currentTarget.value })} placeholder="Ansvarig" />
                    <input
                      class="t-due"
                      type="date"
                      value={isISODate(a.due) ? a.due : ""}
                      onchange={(e) => updateRow(a, { due: e.currentTarget.value })}
                      aria-label="Datum"
                    />
                    {#if a.due && !isISODate(a.due)}<span class="t-due-text" title="Datum som text">{a.due}</span>{/if}
                    {#if dueState(a.due) === "overdue" && !a.done}<span class="t-flag">Förfallen</span>{/if}
                  </div>
                </div>
                <button class="t-del" onclick={() => deleteRow(a)} aria-label="Ta bort" title="Ta bort">×</button>
              </li>
            {/each}
          </ul>
        {/each}
      {/if}
    </div>

  {:else if screen === "deidentify"}
    <div class="layout" class:collapsed={controlsCollapsed}>
      <aside class="sidebar" inert={controlsCollapsed}>
        {@render sourcePicker()}
        <section class="anon-block">
          <h2>Avidentifiering</h2>
          <select class="profile" value={selectedProfile} onchange={(e) => applyProfile(e.currentTarget.value)}>
            {#if selectedProfile==='custom'}<option value="custom">Egen profil</option>{/if}
            {#each PROFILES as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
          </select>
          <ReviewProfiles current={{enabled:ALL_KEYS.filter(k=>enabled[k]),terms,useAi}} disabled={busy} onapply={async(settings)=>{
            enabled=Object.fromEntries(ALL_KEYS.map(k=>[k,settings.enabled.includes(k)]));terms=settings.terms;useAi=!!settings.useAi;selectedProfile='custom';saveWorkspace();
            if(screen==='deidentify')await runDeidentify();else await runAnonymize();
          }} />
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
                  <label><input type="checkbox" bind:checked={enabled[cat.key]} onchange={saveWorkspace} />
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
        {#if error}<div class="banner" class:error={!isWorkCancelled(error)} class:cancelled={isWorkCancelled(error)} role={isWorkCancelled(error)?"status":"alert"}>{error}</div>{/if}
        {#if srcHasTables}<div class="banner warn">Tabelltext ingår i läsordning. Exporten återger texten, inte tabellernas layout.</div>{/if}
        {#if busy}
          <div class="state"><div class="spinner"></div><p class="state-title">{progressMsg || "Arbetar…"}</p><p class="state-sub">Allt körs lokalt.</p></div>
        {:else if analysis}
          <div class="review-head">
            <div class="tabs"><span class="tab on">Avidentifiering</span></div>
            <div class="actions">
              <button class="btn primary" onclick={() => (deidentDoc ? copyAnonDoc() : copyAnon())}>Kopiera</button>
              <button class="btn" onclick={() => openAiCopy("anon")}>Kopiera för AI</button>
              <button class="btn" onclick={() => openExport("anon")} disabled={busy}>Exportera…</button>
            </div>
          </div>
          <div class="meta"><strong>{activeCount}</strong> av {analysis.spans.length} markeras för maskering</div>
          <div class="legend">
            <span class="lg-chip on">så här</span> maskas ·
            <span class="lg-chip off">så här</span> maskas inte ·
            klicka för att slå av/på · klicka ett vanligt ord för att maskera manuellt (blir <span class="lg-manual">understruket</span>)
          </div>
          <!-- svelte-ignore a11y_mouse_events_have_key_events a11y_no_static_element_interactions -->
          <ReviewComparison revision={reviewBasis} rejected={rejectedIds()} stale={reviewStale} approved={reviewApprovedBasis===reviewBasis} onapprove={()=>{reviewApprovedBasis=reviewBasis;saveWorkspace();}}>
          <div class="document" role="presentation" onmouseup={onDocMouseUp}>{#each analysis.segments as seg, i}{#if (i === 0 || seg.para !== analysis.segments[i - 1].para) && speakerForPara(seg.para)}<span class="rev-speaker">{speakerForPara(seg.para)}: </span>{/if}{#if seg.span === null}{#if seg.word}<button class="maskword" onclick={() => openMask(seg)} title="Maskera manuellt">{seg.text}</button>{:else}{seg.text}{/if}{:else}{@const info = analysis.spans[seg.span]}{@const active = isActive(seg.span)}{@const off = !enabled[info.category]}<button
                  class="hit" class:active class:rejected={!active && !off} class:disabled={off} class:manual={info.source === "manual"}
                  style="--c:{colorOf(info.category)}"
                  title={active ? `${info.text} → ${info.replacement} · klicka för att slå av` : `${info.text} · klicka för att slå på`}
                  onclick={() => toggleSpan(seg.span!)}>{seg.text}</button>{/if}{/each}</div>
          </ReviewComparison>
          <div class="reassure">
            <svg viewBox="0 0 24 24" fill="none"><path d="M12 3l8 4v5c0 5-3.4 7.7-8 9-4.6-1.3-8-4-8-9V7l8-4z" stroke="#111214" stroke-width="2"/><path d="M9 12l2 2 4-4" stroke="#3a36b0" stroke-width="2"/></svg>
            Granska alltid träffarna innan du delar. Ingen automatik fångar 100 %.
          </div>
        {:else}
          <div class="state">
            <svg class="state-icon" viewBox="0 0 24 24" fill="none">
              <path d="M12 2.5l7.5 3v4.6c0 5-3.2 7.4-7.5 8.6-4.3-1.2-7.5-3.6-7.5-8.6V5.5L12 2.5z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>
              <path d="M8.5 10.4h5M8.5 13.4h3.4" stroke="#3a36b0" stroke-width="1.7" stroke-linecap="round"/>
            </svg>
            <p class="state-title">Avidentifiera en text</p>
            <p class="state-sub">Klistra in en text eller välj ett dokument till vänster och klicka <strong>Avidentifiera</strong>. Granska träffarna och exportera maskerad text.</p>
          </div>
        {/if}
      </main>
    </div>

  {:else if screen === "summarize"}
    <div class="layout" class:collapsed={controlsCollapsed}>
      <aside class="sidebar" inert={controlsCollapsed}>
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
          {@render modelReference('text')}

          <button class="btn primary block mt" onclick={runSummarizeSource} disabled={busy || qaBusy || actionsBusy || !summaryDownloaded}>
            {summaryDraft ? "Generera om" : "Skapa sammanfattning"}
          </button>
        </section>
      </aside>

      <main class="review">
        {#if error}<div class="banner" class:error={!isWorkCancelled(error)} class:cancelled={isWorkCancelled(error)} role={isWorkCancelled(error)?"status":"alert"}>{error}</div>{/if}
        {#if srcHasTables}<div class="banner warn">Tabelltext ingår i läsordning. Exporten återger texten, inte tabellernas layout.</div>{/if}
        {#if busy}
          <div class="state"><div class="spinner"></div><p class="state-title">{progressMsg || "Arbetar…"}</p><p class="state-sub">Lokal sammanfattning kan ta en stund.</p></div>
        {:else if summaryDraft}
          <div class="review-head">
            <div class="tabs"><span class="tab on">Sammanfattning</span></div>
            <div class="actions">
              <button class="btn primary" onclick={copySummary}>Kopiera</button>
              <button class="btn" onclick={() => openAiCopy("summary")}>Kopiera för AI</button>
              <button class="btn" onclick={() => openExport("summary")} disabled={busy}>Exportera…</button>
            </div>
          </div>
          <div class="banner warn">AI-genererat utkast — kan innehålla fel eller utelämnanden. Granska och redigera innan du delar.</div>
          <textarea disabled={busy || qaBusy || actionsBusy} class="summary-edit" bind:value={summaryDraft} oninput={()=>{summaryAnonymized=false;saveWorkspace();}} aria-label="Sammanfattning – redigerbart utkast" spellcheck="true"></textarea>
        {:else}
          <div class="state">
            <svg class="state-icon" viewBox="0 0 24 24" fill="none">
              <rect x="4.5" y="3" width="15" height="18" rx="2.2" stroke="currentColor" stroke-width="1.5"/>
              <circle cx="8.4" cy="8" r="1.1" fill="#3a36b0"/><path d="M11 8h5.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
              <circle cx="8.4" cy="12" r="1.1" fill="#3a36b0"/><path d="M11 12h5.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
              <circle cx="8.4" cy="16" r="1.1" fill="#3a36b0"/><path d="M11 16h3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
            </svg>
            <p class="state-title">Sammanfatta en text</p>
            <p class="state-sub">Klistra in en text eller välj ett dokument, välj mall och klicka <strong>Skapa sammanfattning</strong>.</p>
          </div>
        {/if}
      </main>
    </div>

  {:else if screen === "meeting"}
    <div class="home">
      <div class="meeting-heading"><h2 class="big-title">Möten</h2><button class="btn" onclick={() => go("transcribe")}>Transkribera ljudfil</button>{#if transcript&&!meetingActive}<button class="btn" onclick={() => tab("overview")}>Tillbaka till {currentJobTitle || "aktuellt möte"}</button>{/if}</div>
      <div class="meeting-card" class:wide={meetingActive}>
        {#if !meetingActive && !meetingBusy}
          <h3>Förbered ett möte</h3><label>Mötesnamn<input aria-label="Mötesnamn" bind:value={meetingName} placeholder="Till exempel Veckomöte med arbetslaget" /></label>
          <label>Mötesmall<select bind:value={meetingPreset} onchange={()=>{const preset=meetingPresets.find(p=>p.name===meetingPreset);if(preset){meetingAgenda=preset.agenda;meetingPeople=preset.people??'';selectedTemplate=preset.summaryTemplate??selectedTemplate;}}}><option value="">Ingen mall</option>{#each meetingPresets as p}<option>{p.name}</option>{/each}</select></label>
          <label>Syfte och agenda<textarea bind:value={meetingAgenda} rows="3" placeholder="Valfritt – kan ändras under mötet"></textarea></label>
          <details><summary>Deltagare och sammanfattningsmall</summary><label>Deltagare, en per rad<textarea aria-label="Deltagare inför mötet" bind:value={meetingPeople} rows="3" placeholder="Alex; Samordnare"></textarea></label><label>Sammanfattningsmall<select bind:value={selectedTemplate}>{#each summaryTemplates as t}<option value={t.id}>{t.label}</option>{/each}</select></label></details>
          <details><summary>Spara som mötesmall</summary><div class="row"><input aria-label="Namn på mötesmall" bind:value={presetName}/><button class="btn" onclick={saveMeetingPreset} disabled={!presetName.trim()}>Spara mall</button></div></details>
          {#if followupActions.length}<div><p>{followupActions.length-excludedFollowup.length} öppna åtgärder följer med från föregående möte.</p>{#each followupActions as a}<label class="ai-toggle"><input type="checkbox" checked={!excludedFollowup.includes(a.id!)} onchange={e=>{excludedFollowup=e.currentTarget.checked?excludedFollowup.filter(id=>id!==a.id):[...excludedFollowup,a.id!];}}/><span>{a.text}</span></label>{/each}</div>{/if}
          <label>Din mikrofon<select aria-label="Välj mikrofon" bind:value={micDevice}><option value="">Windows standardmikrofon</option>{#each devices.filter(d=>d.source==='mic') as d}<option value={d.id}>{d.name}</option>{/each}</select></label>
          <label>Mötesljud<select aria-label="Välj mötesljud" bind:value={systemDevice}><option value="">Windows standardutgång</option>{#each devices.filter(d=>d.source==='system') as d}<option value={d.id}>{d.name}</option>{/each}</select></label>
          {#if meetingDevicesError}<p role="alert">{meetingDevicesError}</p>{/if}
          <button class="btn" onclick={testMeetingSound} disabled={soundTestBusy}>{soundTestBusy?'Testar ljud…':'Testa ljud i 3 sekunder'}</button>{#if soundTestResult}<p role="status">{soundTestResult}</p>{/if}
          <p class="hint big-hint">
            Fångar <strong>din mikrofon</strong> och <strong>mötesljudet</strong> (det som hörs i datorn)
            som två separata spår — så hålls <em>Jag</em> och <em>Mötet</em> isär utan diarisering. Allt körs lokalt.
          </p>
          <div class="consent">
            <svg viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="1.7"/><path d="M12 7.8v5.4" stroke="currentColor" stroke-width="1.9" stroke-linecap="round"/><circle cx="12" cy="16.4" r="1.1" fill="currentColor"/></svg>
            <span>Berätta för deltagarna att du spelar in. Du ansvarar för att inspelningen sker lagligt och med samtycke.</span>
          </div>
          <div class="m-tip">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M4 13v-1a8 8 0 0 1 16 0v1"/><rect x="3" y="13" width="4.5" height="7" rx="1.6"/><rect x="16.5" y="13" width="4.5" height="7" rx="1.6"/></svg>
            <span><strong>Använd hörlurar</strong> för att hålla <em>Jag</em> och <em>Mötet</em> åtskilda. Utan hörlurar fångar mikrofonen även mötesljudet från högtalarna, så den andra personen kan dyka upp under ”Jag”.</span>
          </div>
          <div class="m-fields">
            <div class="m-field">{@render modelReference('speech')}</div>
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

          <button class="btn primary block big mt" onclick={startMeeting} disabled={!selectedDownloaded||soundTestBusy}>Starta inspelning</button>
          <p class="hint">Starta mötet (Teams/Zoom/webbläsare) först, så att mötesljudet spelas upp.</p>
          {#if allJobs.some(j=>j.jobType==='meeting'&&!j.archived)}<section class="recent-meetings"><h3>Senaste möten</h3>{#each allJobs.filter(j=>j.jobType==='meeting'&&!j.archived).slice(0,5) as j}<div class="row"><button class="link" onclick={()=>openJobById(j.id)}>{j.title}</button>{@render workMenu(j)}</div>{/each}</section>{/if}
        {:else if meetingActive || meetingBusy}
          <div class="m-live-head">
            {#if meetingActive}
              <div class="big-rec"><span class="recdot"></span> Spelar in · {fmtTime(meetingElapsed)}</div>
            {:else}
              <!-- Finalisation runs in the background; keep the notes/actions reachable while it does. -->
              <div class="working" aria-live="polite"><span class="working-dot"></span>{progressMsg || "Transkriberar mötet…"}{#if transcribePct !== null} · {transcribePct}%{/if}</div>
            {/if}
            <div class="m-live-toggles">
              <button class="btn small" class:on={meetingShowLive} onclick={toggleLivePane} title="Visa/dölj live-texten">Live-text</button>
              <button class="btn small" class:on={meetingShowNotes} onclick={toggleNotesPane} title="Visa/dölj anteckningar">Anteckningar</button>
            </div>
            {#if meetingActive}
              <button class="btn primary" onclick={stopMeeting}>Stoppa inspelningen</button>
            {/if}
          </div>
          <div class="channel-levels">{#each ['Min mikrofon','Övriga deltagare'] as name,i}<div><strong>{name}</strong><span>{channelLevels[i]?.name||'Väntar på ljudenhet…'}</span><meter aria-label={name+' – ljudnivå'} min="0" max="1" value={Math.min(1,(channelLevels[i]?.peak??0)*5)}></meter><small>{channelLevels[i]?.peak>0.0003?'Ljud registreras':'Ingen ljudnivå just nu'}</small></div>{/each}</div>
          {#if meetingWarning}<p class="banner warn" role="status">{meetingWarning}</p>{/if}
          {#if meetingLagging && meetingActive}
            <div class="banner warn">Transkriberingen släpar efter på den här datorn. All text kommer ikapp när du stoppar — men välj gärna en mindre modell, eller stäng av ”Live-text” nästa gång.</div>
          {/if}
          <div class="m-split" class:single={!(meetingShowLive && meetingShowNotes)}>
            {#if meetingShowLive}
              <div class="m-pane m-pane-live">
                <div class="m-pane-head"><h3>Live-text</h3></div>
                {#if meetingLive && liveUtterances.length}
                  <div class="live-feed" bind:this={liveFeedEl}>
                    {#each liveUtterances as u}
                      <p class="live-line"><span class="live-who {u.source === 'Jag' ? 'me' : 'them'}">{u.source}</span> {u.text}</p>
                    {/each}
                  </div>
                {:else if meetingBusy}
                  <p class="hint" style="text-align:center">Färdigställer transkriptet… det öppnas automatiskt när det är klart. Du kan fortsätta med anteckningarna under tiden.</p>
                {:else if meetingLive}
                  <p class="hint" style="text-align:center">Lyssnar… text dyker upp inom ~10 s när någon talar. Mötesljudet fångas bara medan något faktiskt spelas upp i datorn.</p>
                {:else}
                  <p class="hint" style="text-align:center">Spelar in din röst + mötesljudet. Allt transkriberas när du stoppar.</p>
                {/if}
              </div>
            {/if}
            {#if meetingShowNotes}
              <div class="m-pane m-pane-notes">
                <div class="m-pane-head"><h3>Anteckningar</h3><button class="btn small" onclick={markMeetingTime}>Markera här</button></div>
                {#each bookmarks as b,i (b.id)}<div class="row"><span>{fmtTime(b.time)}</span><input aria-label="Tidsmarkerad anteckning" bind:value={bookmarks[i].text} oninput={saveWorkspace}/></div>{/each}
                <textarea class="m-notes" bind:value={notes} oninput={saveWorkspace} placeholder="Skriv anteckningar medan mötet pågår…"></textarea>
                <form class="ws-add m-add" onsubmit={(e) => { e.preventDefault(); addAction(); }}>
                  <input bind:value={newActionText} placeholder="Ny åtgärd…" />
                  <button class="btn small primary" type="submit" disabled={!newActionText.trim()}>Lägg till</button>
                </form>
                {#if actions.length}
                  <ul class="m-actions">
                    {#each actions as a, i (i)}
                      <li class:done={a.done}>
                        <input type="checkbox" checked={a.done} onchange={() => toggleAction(i)} aria-label="Klar" />
                        <span>{a.text}</span>
                        <button class="x" onclick={() => removeAction(i)} aria-label="Ta bort">×</button>
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>

  {:else if screen === "transcribe"}
  <div class="layout" class:collapsed={controlsCollapsed}>
    <aside class="sidebar" inert={controlsCollapsed}>
      {#if !transcript}
        <section>
          <h2>Ljudfil</h2>
          {#if recording}
            <div class="recording">
              <span class="recdot"></span> Spelar in… {fmtTime(recElapsed)}
              <button class="btn block" onclick={stopRecording}>Stoppa inspelning</button>
            </div>
          {:else if recSaving}
            <div class="working" aria-live="polite"><span class="working-dot"></span>Sparar inspelning…</div>
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
          {@render modelReference('speech')}

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
        <button class="btn block mt" onclick={openProject} disabled={busy}>Öppna projekt…</button>

      {:else if view === "review"}
        <section class="anon-block">
          <h2>Avidentifiering</h2>
          <select class="profile" value={selectedProfile} onchange={(e) => applyProfile(e.currentTarget.value)}>
            {#if selectedProfile==='custom'}<option value="custom">Egen profil</option>{/if}
            {#each PROFILES as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
          </select>
          <ReviewProfiles current={{enabled:ALL_KEYS.filter(k=>enabled[k]),terms,useAi}} disabled={busy} onapply={async(settings)=>{
            enabled=Object.fromEntries(ALL_KEYS.map(k=>[k,settings.enabled.includes(k)]));terms=settings.terms;useAi=!!settings.useAi;selectedProfile='custom';saveWorkspace();
            if(screen==='deidentify')await runDeidentify();else await runAnonymize();
          }} />
          <label class="ai-toggle">
            <input type="checkbox" bind:checked={useAi} />
            <span>Djupare granskning (AI)<em>långsammare, fångar fler ledtrådar</em></span>
          </label>
          <button class="btn primary block" onclick={runAnonymize} disabled={busy}>
            {analysis ? "Kör om avidentifiering" : "Avidentifiera transkript"}
          </button>
        </section>

        {#if analysis}
          <section>
            <h2>Kategorier</h2>
            <ul class="filters">
              {#each CATEGORIES as cat (cat.key)}
                <li>
                  <label><input type="checkbox" bind:checked={enabled[cat.key]} onchange={saveWorkspace} />
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

      {:else if view === "summary"}
        <section class="anon-block">
          <h2>Textbearbetning</h2>
          {@render modelReference('text')}
          <details class="secondary-controls"><summary>Fritt utkast utan källhänvisningar</summary>
          <p class="hint">Källutkast skapas i arbetsytan till höger. Här kan du också skapa ett fritt utkast utan citathänvisningar.</p>
          <select class="profile" bind:value={selectedTemplate}>
            {#each summaryTemplates as t (t.id)}<option value={t.id}>{t.label}</option>{/each}
            <option value="custom">Egen mall / dagordning…</option>
          </select>
          {#if selectedTemplate === "custom"}
            <textarea class="mt" bind:value={customHeadings} rows="4" placeholder="En rubrik per rad, t.ex.&#10;## Närvarande&#10;## Dagordning&#10;## Beslut"></textarea>
          {/if}


          {#if analysis}
            <label class="ai-toggle">
              <input type="checkbox" bind:checked={summaryFromAnon} />
              <span>Använd maskerad text i det fria utkastet<em>gäller inte källutkastets underlag</em></span>
            </label>
          {/if}
          <button class="btn primary block mt" onclick={runSummarize} disabled={busy || qaBusy || actionsBusy || !summaryDownloaded}>
            Skapa fritt utkast
          </button>
          </details>
        </section>

      {:else if view === "qa"}
        <section>
          <h2>Fråga</h2>
          <p class="hint">Ställ frågor om transkriptet i panelen till höger. Svaren bygger bara på texten — inget hittas på.</p>
        </section>

      {:else if (view === "notes" || view === "actions")}
        <section>
          <h2>Anteckningar & åtgärder</h2>
          <p class="hint">Deltagare, fria anteckningar, att-göra-punkter och uppföljning — sparas med projektet. Du kan rätta talarnas namn i transkriptet och hämta in dem som deltagare.</p>
        </section>
        <section>
          <h2>Föreslå åtgärder (AI)</h2>
          <p class="hint">Den lokala AI:n läser transkriptet och föreslår att-göra-punkter.</p>
          {@render modelReference('text')}
          {#if analysis}
            <label class="ai-toggle">
              <input type="checkbox" bind:checked={summaryFromAnon} />
              <span>Använd avidentifierad text<em>maskerade namn/uppgifter</em></span>
            </label>
          {/if}
          <button class="btn primary block mt" onclick={generateActions} disabled={busy || qaBusy || actionsBusy || !summaryDownloaded}>
            {actionsBusy ? "Tar fram…" : "Föreslå åtgärder"}
          </button>
        </section>

      {:else}
        {#if meetingMicWav && meetingSysWav}
          <section class="anon-block">
            <h2>Transkribera om</h2>
            {@render modelReference('speech')}
            <label class="ai-toggle"><input type="checkbox" bind:checked={retranscribeDiarize} /><span>Separera mötesröster automatiskt efteråt</span></label>
            <label class="ai-toggle"><input type="checkbox" bind:checked={retranscribeEchoCancel} /><span>Ta bort eko ur min mik<em>tar bort mötesljudet som läckt in i mikrofonen (om du kört på högtalare)</em></span></label>
            <button class="btn block mt" onclick={retranscribeMeeting} disabled={busy || !selectedDownloaded}>Kör om med vald modell</button>
            <p class="hint">Kör Whisper igen på hela inspelningen — oftast bättre än live, särskilt med en större modell. Talaruppdelningen återställs (kör ”Separera mötesröster” igen efteråt).</p>
          </section>
        {/if}
        {#if meetingSysWav}
          <section class="anon-block">
            <h2>Mötesröster</h2>
            <button class="btn block" onclick={separateMeetingVoices} disabled={busy}>Separera mötesröster</button>
            <p class="hint">Delar upp ”Mötet” i Talare 1, 2 … (din röst förblir ”Jag”).</p>
          </section>
        {/if}
        <section class="anon-block">
          <h2>Rätta återkommande fel</h2>
          <textarea bind:value={correctionInput} rows="3" placeholder="Ett per rad: fel=>rätt&#10;t.ex. kjol=>Tjörn"></textarea>
          <button class="btn block mt" onclick={applyCorrections} disabled={busy || correctionInput.trim() === ""}>
            Tillämpa på hela transkriptet
          </button>
        </section>
        <div class="row">
          <button class="btn grow" onclick={openProject} disabled={busy}>Öppna projekt…</button>
          <button class="btn grow" onclick={saveProject} disabled={busy || !transcript}>
            Spara projektkopia…
          </button>
        </div>
      {/if}
    </aside>

    <main class="review">
      {#if error}<div class="banner" class:error={!isWorkCancelled(error)} class:cancelled={isWorkCancelled(error)} role={isWorkCancelled(error)?"status":"alert"}>{error}</div>{/if}
      {#if analysis?.warnings.length}{#each analysis.warnings as w}<div class="banner warn">{w}</div>{/each}{/if}

      {#if busy && !transcript}
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
      {:else if currentJobPending && !(transcript?.utterances?.length) && !["notes","actions","overview"].includes(view)}
        <div class="state">
          {#if bgMeetings.some((m) => m.id === currentJobId)}
            <div class="spinner"></div>
            <p class="state-title">Transkriberas i bakgrunden…</p>
            <p class="state-sub">Transkriptet dyker upp här automatiskt när det är klart. Ljudet är sparat. Om du stänger appen avbryts bearbetningen; öppna mötet och välj Transkribera om för att fortsätta.</p>
          {:else}
            <svg class="state-icon" viewBox="0 0 24 24" fill="none"><circle cx="12" cy="12" r="9" stroke="#b45309" stroke-width="1.7"/><path d="M12 7.4v5l3 2" stroke="#b45309" stroke-width="1.7" stroke-linecap="round"/></svg>
            <p class="state-title">Transkriberingen är inte klar</p>
            <p class="state-sub">Det här mötet avbröts innan transkriberingen blev klar. Ljudet är sparat.</p>
            {#if meetingMicWav && meetingSysWav}
              {#if selectedDownloaded}<button class="btn primary mt" onclick={retranscribeMeeting} disabled={busy}>Transkribera om</button>{:else}<p class="hint">Hämta talmodellen under Modeller på datorn för att transkribera om mötet.</p>{/if}
            {/if}
          {/if}
        </div>
      {:else if !transcript && !["notes","actions","overview"].includes(view)}
        <div class="state">
          <svg class="state-icon" viewBox="0 0 24 24" fill="none">
            <path d="M4 9v6M7 6.5v11M10 9.5v5" stroke="#3a36b0" stroke-width="1.5" stroke-linecap="round"/>
            <path d="M14 8.5h6M14 12h6M14 15.5h4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          </svg>
          <p class="state-title">Välj en ljudfil och transkribera</p>
          <p class="state-sub">Välj modell och språk, slå på <strong>diarisering</strong> för att skilja talare åt, och klicka <strong>Transkribera</strong>. Sedan kan du avidentifiera och exportera.</p>
        </div>
      {:else}
        {#if busy || qaBusy || actionsBusy}
          <div class="working" role="status" aria-live="polite"><span class="working-dot"></span>{progressMsg || "Arbetar…"}{#if transcribePct !== null} · {transcribePct}%{/if}</div>
        {/if}
        <div class="review-head actions-only">
          <div class="actions">
            {#if currentJobType==='meeting'}<button class="btn" onclick={()=>openExport('meeting')}>Exportera mötesunderlag</button>{/if}
            {#if view === "overview"}<span class="hint">Välj innehåll och granska före export.</span>
            {:else if view === "summary" && summaryDraft}
              <button class="btn primary" onclick={copySummary}>Kopiera</button>
              <button class="btn" onclick={() => openAiCopy("summary")}>Kopiera för AI</button>
              <button class="btn" onclick={() => openExport("summary")} disabled={busy}>Exportera…</button>
            {:else if view === "review" && analysis}
              <button class="btn primary" onclick={copyAnon}>Kopiera</button>
              <button class="btn" onclick={() => openAiCopy("anon")}>Kopiera för AI</button>
              <button class="btn" onclick={() => openExport("anon")} disabled={busy}>Exportera…</button>
            {:else if (view === "notes" || view === "actions")}
              <button class="btn primary" onclick={copyWorkspace} disabled={!wsHasContent}>Kopiera</button>
              <button class="btn" onclick={() => openExport("notes")} disabled={!wsHasContent || busy}>Exportera…</button>
            {:else}
              <button class="btn primary" onclick={copyTranscript}>Kopiera</button>
              <button class="btn" onclick={() => openAiCopy("transcript")}>Kopiera för AI</button>
              <button class="btn" onclick={() => openExport("transcript")} disabled={busy}>Exportera…</button>
            {/if}
          </div>
        </div>

        {#if audioSrc}
          <div class="player">
            {#if meetingMicWav}<label class="track-choice">Lyssna på<select aria-label="Ljudspår" bind:value={playbackTrack} onchange={()=>{audioEl?.pause();playing=false;}}><option value="mix">Båda spåren</option><option value="mic">Min mikrofon</option><option value="system">Övriga deltagare</option></select></label>{/if}
            <select aria-label="Uppspelningshastighet" bind:value={playbackRate} onchange={()=>{if(audioEl)audioEl.playbackRate=Number(playbackRate);}}><option value={0.75}>0,75×</option><option value={1}>1×</option><option value={1.25}>1,25×</option><option value={1.5}>1,5×</option><option value={2}>2×</option></select>
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
              aria-label="Position i inspelningen"
              step="0.01"
              value={currentTime}
              oninput={(e) => seekTo(+e.currentTarget.value)}
            />
            <audio
              bind:this={audioEl}
              src={audioSrc}
              preload="metadata"
              onloadedmetadata={()=>{if(audioEl){audioEl.currentTime=Math.min(currentTime,audioEl.duration||0);audioEl.playbackRate=Number(playbackRate);}}}
              ontimeupdate={() => (currentTime = audioEl?.currentTime ?? 0)}
              onplay={() => (playing = true)}
              onpause={() => {playing = false;if(!restoringJob)saveWorkspace();}}
              onended={() => (playing = false)}
            ></audio>
          </div>
        {/if}

        {#if view === "overview"}
          <section class="meeting-overview">
            <div class="overview-heading"><div><h2>{currentJobTitle}</h2><p>{currentJobPending?'Transkriptet bearbetas. Dina anteckningar och åtgärder är tillgängliga.':'Mötets innehåll och nästa steg.'}</p></div><button class="btn" onclick={prepareFollowup} disabled={meetingActive}>Förbered uppföljningsmöte</button></div>
            {#if meetingWarning}<p class="banner warn" role="status">{meetingWarning}</p>{/if}
            {#if currentJobPending}<p role="status">{bgMeetings.find(m=>m.id===currentJobId)?.msg??'Bearbetningen avbröts. Öppna Transkript och välj Transkribera om.'}</p>{/if}
            <div class="overview-next"><button onclick={()=>tab('transcript')}><strong>Transkript</strong><span>{transcript?.utterances.length??0} avsnitt</span></button><button onclick={()=>tab('notes')}><strong>Anteckningar</strong><span>{bookmarks.length} tidsmarkeringar</span></button><button onclick={()=>tab('actions')}><strong>Beslut och åtgärder</strong><span>{decisions.length} beslut · {actions.filter(a=>!a.done).length} öppna åtgärder</span></button></div>
            <div class="overview-grid"><section><h3>Syfte och agenda</h3><textarea aria-label="Mötets agenda" bind:value={agenda} oninput={saveWorkspace} rows="6" placeholder="Vad ska mötet handla om?"></textarea></section>
            <section><h3>Deltagare</h3>{#each participants as p,i}<div class="row"><input aria-label="Deltagarnamn" bind:value={participants[i].name} oninput={saveWorkspace}/><input aria-label="Deltagarens roll" bind:value={participants[i].role} oninput={saveWorkspace}/><button class="link" onclick={()=>removeParticipant(i)} aria-label="Ta bort deltagare">×</button></div>{/each}<button class="btn small" onclick={addParticipant}>Lägg till deltagare</button><button class="btn small" onclick={seedParticipantsFromSpeakers}>Från talare</button>
              <label class="followup-date">Följ upp den<input type="date" bind:value={followup} onchange={saveWorkspace}/></label></section></div>
            {#if followupFrom}<button class="link" onclick={()=>followupFrom&&openJobById(followupFrom)}>Öppna föregående möte</button>{/if}
            {#if summaryDraft}<section><h3>Sammanfattning – utkast</h3><p class="summary-preview">{summaryDraft}</p><button class="btn" onclick={()=>tab('summary')}>Granska och redigera</button></section>{:else}<button class="btn" onclick={()=>tab('summary')} disabled={currentJobPending}>Skapa sammanfattning</button>{/if}
          </section>
        {:else if view === "transcript"}
          {#if meetingWarning}<p class="banner warn" role="status">{meetingWarning}</p>{/if}
          <div class="t-toolbar">
            <button class="btn small" class:on={editMode} onclick={() => (editMode = !editMode)} title="Växla mellan att spela upp och att rätta text">
              {editMode ? "✓ Redigerar" : "Redigera"}
            </button>
            <button class="btn small" onclick={undoEdit} disabled={!undoStack.length}>Ångra</button>
            <span class="meta">
              {transcript?.utterances.length??0} segment · modell {transcript?.model}{transcript?.diarized ? " · diariserad" : ""} ·
              {editMode ? "klicka en rad för att rätta · byt talare · ta bort" : "klicka tid för att spela · klicka/dubbelklicka text för att rätta"}
            </span>
          </div>
          {#key currentJobId}
            <TranscriptView bind:this={transcriptView} utterances={transcript?.utterances??[]} {speakerLabels} {speakerOptions} {playing} {currentTime} {editMode} {editingIdx} bind:editText
              onseek={seekTo} onedit={startEdit} oncommit={commitEdit} oncancel={cancelEdit} onrename={renameSpeaker} onspeaker={setSpeaker} ondelete={deleteUtterance} />
          {/key}
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
              <button class="btn primary" type="submit" disabled={busy || qaBusy || actionsBusy || !qaQuestion.trim() || !summaryDownloaded}>{qaBusy ? "…" : "Fråga"}</button>
            </form>
            {@render modelReference('text')}
          </div>
        {:else if view === "review"}
          {#if analysis}
          <div class="meta"><strong>{activeCount}</strong> av {analysis.spans.length} markeras för maskering</div>
          <div class="legend">
            <span class="lg-chip on">så här</span> maskas ·
            <span class="lg-chip off">så här</span> maskas inte ·
            klicka för att slå av/på · klicka ett vanligt ord för att maskera manuellt (blir <span class="lg-manual">understruket</span>)
          </div>
          <!-- svelte-ignore a11y_mouse_events_have_key_events a11y_no_static_element_interactions -->
          <ReviewComparison revision={reviewBasis} rejected={rejectedIds()} stale={reviewStale} approved={reviewApprovedBasis===reviewBasis} onapprove={()=>{reviewApprovedBasis=reviewBasis;saveWorkspace();}}>
          <div class="document" role="presentation" onmouseup={onDocMouseUp}>{#each analysis.segments as seg, i}{#if (i === 0 || seg.para !== analysis.segments[i - 1].para) && speakerForPara(seg.para)}<span class="rev-speaker">{speakerForPara(seg.para)}: </span>{/if}{#if seg.span === null}{#if seg.word}<button class="maskword" onclick={() => openMask(seg)} title="Maskera manuellt">{seg.text}</button>{:else}{seg.text}{/if}{:else}{@const info = analysis.spans[seg.span]}{@const active = isActive(seg.span)}{@const off = !enabled[info.category]}<button
                  class="hit" class:active class:rejected={!active && !off} class:disabled={off} class:manual={info.source === "manual"}
                  style="--c:{colorOf(info.category)}"
                  title={active ? `${info.text} → ${info.replacement} · klicka för att slå av` : `${info.text} · klicka för att slå på`}
                  onclick={() => toggleSpan(seg.span!)}>{seg.text}</button>{/if}{/each}</div>
          </ReviewComparison>
          <div class="reassure">
            <svg viewBox="0 0 24 24" fill="none"><path d="M12 3l8 4v5c0 5-3.4 7.7-8 9-4.6-1.3-8-4-8-9V7l8-4z" stroke="#111214" stroke-width="2"/><path d="M9 12l2 2 4-4" stroke="#3a36b0" stroke-width="2"/></svg>
            Granska alltid träffarna innan du delar. Ingen automatik fångar 100 %.
          </div>
          {:else}
          <div class="empty-view">
            <h3>Avidentifiera transkriptet</h3>
            <p class="hint">Välj kategorier (och ev. djupare AI-granskning) och klicka <strong>Avidentifiera transkript</strong> i panelen till vänster — träffarna dyker upp här för granskning.</p>
          </div>
          {/if}
        {:else if view === "summary"}
          <GroundedDraft disabled={qaBusy || actionsBusy} value={groundedWork} sources={groundedSources} context={groundedContext} model={selectedSummaryModel} canListen={!!audioPath} bind:busy before={checkpointWork} onchange={saveGrounded} onseek={seekGrounded} onaction={addGroundedAction} ondecision={addGroundedDecision} onuse={useGrounded} />
          {#if summaryDraft}
          <div class="banner warn">AI-genererat utkast — kan innehålla fel eller utelämnanden. Granska och redigera innan du delar.</div>
          <textarea disabled={busy || qaBusy || actionsBusy} class="summary-edit" bind:value={summaryDraft} oninput={()=>{summaryAnonymized=false;saveWorkspace();}} aria-label="Sammanfattning – redigerbart utkast" spellcheck="true"></textarea>
          {:else}
          <div class="empty-view">
            <h3>Skapa en sammanfattning</h3>
            <p class="hint">Välj mall och modell och klicka <strong>Skapa sammanfattning</strong> i panelen till vänster — utkastet dyker upp här för redigering.</p>
          </div>
          {/if}
        {:else if (view === "notes" || view === "actions")}
          {#if view==='notes'}<section class="meeting-marks"><h3>Tidsmarkerade anteckningar</h3><button class="btn small" onclick={markMeetingTime} disabled={!audioSrc}>Markera här</button>
            {#each bookmarks as b,i (b.id)}<div class="row"><button class="btn small" onclick={()=>seekTo(b.time)}>{fmtTime(b.time)}</button><input aria-label="Tidsmarkerad anteckning" bind:value={bookmarks[i].text} oninput={saveWorkspace}/><button class="link" onclick={()=>{bookmarks=bookmarks.filter(x=>x.id!==b.id);saveWorkspace();}} aria-label="Ta bort tidsmarkering">×</button></div>{/each}</section>{/if}
          {#if view==='actions'}<section class="meeting-decisions"><details><summary>Fritt åtgärdsförslag</summary><p class="hint">AI-förslagen läggs i listan för manuell kontroll.</p><button class="btn" onclick={generateActions} disabled={busy||qaBusy||actionsBusy||currentJobPending||!summaryDownloaded}>Föreslå åtgärder</button></details><div class="row"><h3>Beslut</h3><button class="btn" onclick={()=>tab("summary")} disabled={currentJobPending}>Föreslå beslut och åtgärder från källan</button></div>
            {#each decisions as d,i (d.id)}<div class="decision"><input aria-label="Beslut" bind:value={decisions[i].text} oninput={saveWorkspace}/>{#if d.quote}<blockquote>{d.quote}</blockquote>{#if d.start!=null}<button class="link" onclick={()=>seekTo(d.start!)}>Lyssna vid {fmtTime(d.start)}</button>{/if}{/if}<button class="link" onclick={()=>{decisions=decisions.filter(x=>x.id!==d.id);saveWorkspace();}}>Ta bort beslut</button></div>{/each}
            <form class="row" onsubmit={e=>{e.preventDefault();addDecision();}}><input aria-label="Nytt beslut" bind:value={decisionText} placeholder="Vad beslutade ni?"/><button class="btn" disabled={!decisionText.trim()}>Lägg till beslut</button></form></section>{/if}
          {#snippet actionsBody()}
            {#if actionPasteOpen}
              <div class="ws-paste">
                <textarea bind:value={actionPasteText} rows="4" placeholder="Klistra in en lista — en åtgärd per rad. ”- ”, ”• ”, ”1.” och ”[ ]” funkar."></textarea>
                <div class="row">
                  <button class="btn small primary" onclick={applyActionPaste} disabled={!actionPasteText.trim()}>Lägg till</button>
                  <button class="btn small" onclick={() => { actionPasteOpen = false; actionPasteText = ""; }}>Avbryt</button>
                </div>
              </div>
            {/if}
            {#if actions.length}
              <ul class="ws-actions">
                {#each actions as a, i (i)}
                  <li class="ws-action" class:done={a.done}>
                    <input class="ws-check" type="checkbox" checked={a.done} onchange={() => toggleAction(i)} aria-label="Klar" />
                    <div class="ws-action-main">
                      <input class="ws-atext" bind:value={actions[i].text} oninput={saveWorkspace} placeholder="Åtgärd" />
                      {#if a.source}<details><summary>Visa källa</summary><blockquote>{a.source.quote}</blockquote>{#if a.source.start!==null}<button class="link" onclick={()=>listenActionSource(a.source!)}>Lyssna på källan</button>{/if}</details>{/if}
                      <div class="ws-ameta">
                        <input class="ws-assignee" bind:value={actions[i].assignee} oninput={saveWorkspace} placeholder="Ansvarig" />
                        <input class="ws-due" type="date" bind:value={actions[i].due} onchange={saveWorkspace} title="Klart till" />
                      </div>
                    </div>
                    <div class="ws-action-ctrls">
                      <button class="mini" onclick={() => moveAction(i, -1)} disabled={i === 0} aria-label="Flytta upp">↑</button>
                      <button class="mini" onclick={() => moveAction(i, 1)} disabled={i === actions.length - 1} aria-label="Flytta ner">↓</button>
                      <button class="x" onclick={() => removeAction(i)} aria-label="Ta bort">×</button>
                    </div>
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="hint">Inga åtgärder ännu. Skriv en nedan, klistra in en lista, eller låt AI:n föreslå från transkriptet.</p>
            {/if}
            <form class="ws-add" onsubmit={(e) => { e.preventDefault(); addAction(); }}>
              <input bind:value={newActionText} placeholder="Ny åtgärd…" />
              <button class="btn small primary" type="submit" disabled={!newActionText.trim()}>Lägg till</button>
            </form>
          {/snippet}

          <div class="ws" class:notes-only={view==='notes'} class:actions-only={view==='actions'}>
            {#if maximized === "notes"}
              <section class="ws-card ws-max">
                <header class="ws-head">
                  <h3>Anteckningar</h3>
                  <button class="btn small" onclick={() => (maximized = null)}>Minimera</button>
                </header>
                <textarea class="ws-notes max" bind:value={notes} oninput={saveWorkspace} placeholder="Fria anteckningar från mötet…"></textarea>
              </section>
            {:else if maximized === "actions"}
              <section class="ws-card ws-max">
                <header class="ws-head">
                  <h3>Att göra {#if actions.length}<span class="ws-badge">{actions.filter((a) => a.done).length}/{actions.length}</span>{/if}</h3>
                  <div class="ws-head-actions">
                    <button class="btn small" onclick={() => (actionPasteOpen = !actionPasteOpen)}>Klistra in</button>
                    <button class="btn small" onclick={() => (maximized = null)}>Minimera</button>
                  </div>
                </header>
                <div class="ws-scroll">{@render actionsBody()}</div>
              </section>
            {:else}
              <div class="ws-grid">
                <div class="ws-col-main">
                  <section class="ws-card notes-card">
                    <header class="ws-head">
                      <h3><svg class="ws-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M5 4h10l4 4v12H5z"/><path d="M9 11h6M9 15h6"/></svg> Anteckningar</h3>
                      <button class="btn small" onclick={() => (maximized = "notes")} title="Maximera">Maximera</button>
                    </header>
                    <textarea class="ws-notes" bind:value={notes} oninput={saveWorkspace} placeholder="Fria anteckningar från mötet…"></textarea>
                  </section>

                  <section class="ws-card actions-card">
                    <header class="ws-head">
                      <h3><svg class="ws-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><path d="M20 7.5V18a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h8"/><path d="M9 11l3 3 8-8"/></svg> Att göra {#if actions.length}<span class="ws-badge">{actions.filter((a) => a.done).length}/{actions.length}</span>{/if}</h3>
                      <div class="ws-head-actions">
                        <button class="btn small" onclick={() => (actionPasteOpen = !actionPasteOpen)}>Klistra in</button>
                        <button class="btn small" onclick={() => (maximized = "actions")} title="Maximera">Maximera</button>
                      </div>
                    </header>
                    {@render actionsBody()}
                  </section>
                </div>

                <div class="ws-col-side facts-card">
                  <section class="ws-card">
                    <header class="ws-head">
                      <h3><svg class="ws-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><circle cx="9" cy="8" r="3"/><path d="M3.5 19c0-3 2.5-5 5.5-5s5.5 2 5.5 5"/><path d="M16 5.5a3 3 0 0 1 0 5M17.5 14c2.2.5 3.5 2.2 3.5 5"/></svg> Deltagare</h3>
                      <div class="ws-head-actions">
                        {#if speakerOptions.length}<button class="btn small" onclick={seedParticipantsFromSpeakers} title="Hämta namn från transkriptets talare">Från talare</button>{/if}
                        <button class="btn small" onclick={addParticipant}>+ Lägg till</button>
                      </div>
                    </header>
                    {#if participants.length}
                      <div class="ws-people">
                        {#each participants as p, i (i)}
                          <div class="ws-person">
                            <div class="ws-person-top">
                              <input class="ws-pname" bind:value={participants[i].name} oninput={saveWorkspace} placeholder="Namn" />
                              <button class="x" onclick={() => removeParticipant(i)} aria-label="Ta bort">×</button>
                            </div>
                            <input class="ws-prole" bind:value={participants[i].role} oninput={saveWorkspace} placeholder="Roll (valfritt)" />
                          </div>
                        {/each}
                      </div>
                    {:else}
                      <p class="hint">Lägg till deltagare manuellt, eller hämta namnen från transkriptets talare.</p>
                    {/if}
                  </section>

                  <section class="ws-card">
                    <header class="ws-head"><h3><svg class="ws-ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7"><rect x="4" y="5" width="16" height="15" rx="2"/><path d="M4 9.5h16M8 3v4M16 3v4"/></svg> Uppföljning</h3></header>
                    <div class="ws-followup">
                      <label>Uppföljningsdatum
                        <input type="date" bind:value={followup} onchange={saveWorkspace} />
                      </label>
                      {#if followup}<button class="link" onclick={() => { followup = ""; saveWorkspace(); }}>rensa</button>{/if}
                    </div>
                  </section>
                </div>
              </div>
            {/if}
          </div>
        {/if}
      {/if}
    </main>
  </div>
  {/if}
  </div>
</div>

{#if versionsOpen && currentJobId}
  <VersionsDialog jobId={currentJobId} original={originalText()} onrestore={restoreVersion} onclose={() => versionsOpen=false} />
{/if}
{#if exportOpen}
  <ExportDialog choices={exportChoices} initial={exportInitial} prepare={prepareExport} onsave={saveExportSnapshot} onclose={() => exportOpen = false} />
{/if}

{#if toast}
  <div class="toast"><span class="accentbar"></span><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6"><path d="M5 13l4 4L19 7"/></svg>{toast}</div>
{/if}

{#if aiOpen}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="modal-backdrop" onclick={() => (aiOpen = false)} role="presentation">
    <div class="modal ai-modal" role="dialog" aria-modal="true" onclick={(e) => e.stopPropagation()}>
      <h3 class="modal-title">Kopiera för AI</h3>

      {#if aiDeid}
        <p class="ai-status ok">Valda maskeringar tillämpas. Kontrollera texten innan du skickar den till en extern tjänst.</p>
      {:else}
        <div class="banner warn">
          Den här texten är <strong>inte avidentifierad</strong>. Klistrar du in den i en extern AI-tjänst
          skickas personuppgifter dit. Avidentifiera först om du kan.
        </div>
        <label class="ai-confirm">
          <input type="checkbox" bind:checked={aiUseOriginal} />
          Jag förstår och vill kopiera originaltexten ändå
        </label>
      {/if}

      <label class="modal-field">Uppgift
        <select bind:value={aiPromptId} onchange={selectAiPrompt}>
          {#each aiPromptBank as p (p.id)}<option value={p.id}>{p.label}</option>{/each}
        </select>
      </label>

      <label class="modal-field">Prompt (redigerbar)
        <textarea bind:value={aiPromptDraft} rows="5" placeholder="Skriv din egen instruktion till AI:n…"></textarea>
      </label>
      <div class="ai-prompt-actions">
        <button class="link" onclick={resetAiPrompt}>Återställ</button>
        <button class="link" onclick={saveAiPromptDraft}>Spara som standard</button>
      </div>

      <label class="modal-field">Kontext (valfritt)
        <input bind:value={aiContext} placeholder="t.ex. Elevhälsomöte om läs- och skrivstöd" />
      </label>

      <p class="hint">
        Prompt{aiDeid ? " + avidentifierad text" : " + text"} kopieras ({aiText.length.toLocaleString("sv-SE")} tecken).
        Klistra sedan in i valfri AI (ChatGPT, Claude, Gemini, Copilot …).
      </p>

      <div class="ai-open">
        <span class="ai-open-label">Öppna direkt i:</span>
        {#each AI_TARGETS as t (t.id)}
          <button class="btn small" onclick={() => openInAi(t)} disabled={!aiDeid && !aiUseOriginal}>{t.label}</button>
        {/each}
      </div>

      <div class="modal-actions">
        <button class="btn" onclick={() => (aiOpen = false)}>Avbryt</button>
        <button class="btn primary" onclick={copyForAi} disabled={!aiDeid && !aiUseOriginal}>Kopiera</button>
      </div>
    </div>
  </div>
{/if}

{#if maskTarget}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
  <div class="modal-backdrop" onclick={() => (maskTarget = null)} role="presentation">
    <div class="modal" role="dialog" aria-modal="true" onclick={(e) => e.stopPropagation()}>
      <h3 class="modal-title">Maskera ”{maskTarget.text.length > 60 ? maskTarget.text.slice(0, 60) + "…" : maskTarget.text}”</h3>
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

{#if selPending}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <button
    class="sel-mask-btn"
    style="left:{selPending.x}px; top:{selPending.y}px"
    onmousedown={(e) => e.preventDefault()}
    onclick={maskSelection}
  >Maskera markering</button>
{/if}

<style>
  :global(:root) {
    --ink: #242431; --muted: #626275; --faint: #626275; --bg: #ffffff;
    --nav-bg: #f3f3f8; --accent-soft: #ededff;
    --canvas: #ffffff; /* pure white */
    --line: #e8e8eb; --line-2: #dadadf; --accent: #3a36b0; --accent-press: #2e2b8f; /* deep ink-indigo */
    --shadow-sm: none; --shadow-md: none; --shadow-lg: none; /* editorial: depth from hairlines + space, not shadows */
  }
  :global(body) { margin: 0; font-family: "Archivo", system-ui, sans-serif; color: var(--ink); background: var(--canvas); -webkit-font-smoothing: antialiased; }
  .app { height:100dvh; display:grid; grid-template-columns:204px minmax(0,1fr); }
  .app-content { min-width:0; min-height:0; display:flex; flex-direction:column; overflow:auto; }

  .workspace-header { display: flex; align-items: flex-end; gap: 15px; padding: 20px 30px 16px; border-bottom: 1px solid var(--line); background: var(--bg); }
  .spacer { flex: 1; }

  .layout { min-height:0; flex: 1; display: grid; grid-template-columns: 280px minmax(0,1fr); overflow: hidden; transition: grid-template-columns .16s ease; }
  .layout.collapsed { grid-template-columns: 0 1fr; }
  .layout.collapsed .sidebar { padding: 0; border-right: none; overflow: hidden; }
  .layout.collapsed .ws { max-width: none; } /* sidebar hidden → let the workspace use the full width */
  .sidebar { padding: 22px 24px; overflow: auto; border-right: 1px solid var(--line); }
  section { margin-bottom: 22px; }
  h2 { font-size: 14px; letter-spacing: 0; text-transform: none; color: var(--ink); margin: 0 0 11px; font-weight: 600; }
  .hint { font-size: 13px; color: var(--faint); margin: 6px 0 0; }

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
  .btn svg { width: 17px; height: 17px; flex: none; }
  .btn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .btn.primary:hover:not(:disabled) { background: var(--accent-press); border-color: var(--accent-press); }
  .btn:disabled, .btn.primary:disabled { background: var(--canvas); color: var(--faint); border-color: var(--line); box-shadow: none; filter: none; cursor: default; }
  .btn.block { width: 100%; }
  .btn.mt { margin-top: 8px; }
  .btn.big { padding: 12px; font-size: 15px; margin-bottom: 22px; }
  .link { border: none; background: none; color: var(--accent); cursor: pointer; font: inherit; font-size: 13px; padding: 0 2px; }
  .x { border: none; background: none; color: var(--muted); cursor: pointer; font-size: 16px; line-height: 1; padding: 0 2px; }

  .file-chip { display: flex; justify-content: space-between; align-items: center; gap: 8px; background: color-mix(in srgb, var(--accent) 5%, var(--bg)); border: 1px solid color-mix(in srgb, var(--accent) 18%, var(--bg)); border-radius: 3px; padding: 9px 11px; font-size: 13.5px; }
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
  .review-head.actions-only { justify-content: flex-end; }
  .tabs { display: flex; gap: 4px; }
  .tab { font: inherit; font-size: 14px; font-weight: 600; border: none; background: none; color: var(--faint); cursor: pointer; padding: 6px 4px; border-bottom: 2px solid transparent; }
  .tab.on { color: var(--ink); border-bottom-color: var(--accent); }
  .tab:disabled { opacity: .4; cursor: default; }
  .actions { display: flex; gap: 7px; flex-wrap: wrap; }
  .meta { font-size: 13px; color: var(--muted); margin-bottom: 12px; }
  .meta strong { color: var(--ink); font-weight: 700; }

  .progress { width: 220px; height: 7px; background: var(--line); border-radius: 4px; margin: 4px auto 10px; overflow: hidden; }
  .progress-bar { height: 100%; background: var(--accent); transition: width .25s; }

  .row { display: flex; gap: 8px; margin: 8px 0 22px; }
  .grow { flex: 1; }

  .player { display: flex; align-items: center; gap: 12px; padding: 10px 14px; margin-bottom: 14px; background: color-mix(in srgb, var(--accent) 5%, var(--bg)); border: 1px solid color-mix(in srgb, var(--accent) 18%, var(--bg)); border-radius: 3px; }
  .play { flex: none; width: 36px; height: 36px; border-radius: 50%; border: none; background: var(--accent); color: #fff; cursor: pointer; display: inline-flex; align-items: center; justify-content: center; }
  .play svg { width: 16px; height: 16px; }
  .pt { font-size: 12px; color: var(--muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .seek { flex: 1; accent-color: var(--accent); cursor: pointer; }

  .document { flex: 1; overflow: auto; white-space: pre-wrap; line-height: 2.1; font-size: 16px; max-width: 82ch; background: var(--bg); border: 1px solid var(--line); border-radius: 3px; box-shadow: var(--shadow-sm); padding: 20px 26px; }
  .summary-edit { flex: 1; width: 100%; box-sizing: border-box; resize: none; font: inherit; font-size: 15px; line-height: 1.7; color: var(--ink); border: 1px solid var(--line); border-radius: 3px; box-shadow: var(--shadow-sm); padding: 20px 24px; max-width: 84ch; }
  .summary-edit:focus { outline: none; border-color: var(--accent); }
  .hit { border: none; background: color-mix(in srgb, var(--c) 14%, transparent); font: inherit; line-height: inherit; cursor: pointer; padding: 0 2px 1px; border-radius: 3px; border-bottom: 2px solid var(--c); transition: background .14s; color: inherit; }
  .hit:hover { background: color-mix(in srgb, var(--c) 30%, transparent); }
  .hit.rejected { background: none; border-bottom: 2px dotted var(--faint); text-decoration: line-through; color: var(--faint); }
  .hit.disabled { background: none; border-bottom: none; cursor: default; }
  .legend { font-size: 12px; color: var(--muted); margin: 2px 0 10px; line-height: 1.8; }
  .lg-chip { padding: 0 4px; border-radius: 3px; }
  .lg-chip.on { background: color-mix(in srgb, var(--accent) 16%, transparent); border-bottom: 2px solid var(--accent); color: var(--ink); }
  .lg-chip.off { text-decoration: line-through; color: var(--faint); }
  .lg-manual { text-decoration: underline; text-decoration-thickness: 2px; text-underline-offset: 2px; }

  .reassure { margin-top: 16px; padding-top: 14px; border-top: 1px solid var(--line); font-size: 12.5px; color: var(--muted); display: flex; align-items: center; gap: 9px; }
  .reassure svg { width: 15px; height: 15px; color: var(--accent); }

  .state { margin: auto; text-align: center; color: var(--muted); max-width: 400px; padding: 30px; }
  .state-icon { width: 44px; height: 44px; color: var(--faint); margin-bottom: 14px; }
  .state-title { font-family: "Instrument Serif", serif; font-size: 29px; line-height: 1.12; color: var(--ink); margin: 0 0 8px; letter-spacing: -.01em; }
  .state-sub { font-size: 14px; margin: 0; line-height: 1.6; }
  .spinner { width: 34px; height: 34px; border: 3px solid var(--line); border-top-color: var(--accent); border-radius: 50%; margin: 0 auto 16px; animation: spin .8s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  .banner { border-radius: 3px; padding: 10px 13px; margin-bottom: 12px; font-size: 13.5px; }
  .banner.cancelled { background: var(--bg); color: var(--ink); border: 1px solid var(--line); }
  .banner.error { background: #fef2f2; color: #b91c1c; border: 1px solid #fecaca; }
  .banner.warn { background: #fffbeb; color: #92400e; border: 1px solid #fde68a; }

  .toast { position: fixed; bottom: 28px; left: 50%; transform: translateX(-50%); background: var(--ink); color: #fff; padding: 12px 18px 12px 20px; border-radius: 3px; font-size: 13.5px; font-weight: 500; display: flex; align-items: center; gap: 9px; box-shadow: 0 18px 44px rgba(0,0,0,.28); overflow: hidden; }
  .toast svg { width: 16px; height: 16px; color: var(--accent); }
  .toast .accentbar { position: absolute; left: 0; top: 0; bottom: 0; width: 3px; background: var(--accent); }

  /* ---- task shell: brand-as-home button + discreet top nav ---- */

  /* ---- polish: interactions, focus, selection, scrollbars (within the existing look) ---- */
  .btn:active:not(:disabled) { transform: translateY(1px); }
  .link:hover { text-decoration: underline; }
  .x:hover { color: var(--ink); }
  textarea:focus, select.profile:focus { outline: none; border-color: var(--accent); }
  textarea:focus-visible, select.profile:focus-visible, input:focus-visible {
    outline: none; border-color: var(--accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent);
  }
  :global(:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  :global(::selection) { background: color-mix(in srgb, var(--accent) 22%, transparent); }
  :global(::-webkit-scrollbar) { width: 12px; height: 12px; }
  :global(::-webkit-scrollbar-thumb) { background-color: color-mix(in srgb, var(--ink) 20%, transparent); border-radius: 3px; border: 3px solid var(--bg); background-clip: padding-box; }
  :global(::-webkit-scrollbar-thumb:hover) { background-color: color-mix(in srgb, var(--ink) 34%, transparent); }
  :global(::-webkit-scrollbar-track) { background: transparent; }

  /* ---- home / history ---- */
  .home { flex: 1; overflow: auto; padding: 46px 40px 60px; max-width: 920px; width: 100%; margin: 0 auto; box-sizing: border-box; }
  .big-title { font-family: "Instrument Serif", serif; font-weight: 400; font-size: 42px; line-height: 1.06; color: var(--ink); margin: 0 0 30px; letter-spacing: -.01em; text-transform: none; }

  .recent { margin-top: 34px; }
  .recent h2 { font-size: 14px; letter-spacing: 0; text-transform: none; color: var(--ink); margin: 0 0 11px; font-weight: 600; }
  .job-strip, .job-list { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .job-item { display: flex; align-items: center; gap: 6px; }
  .job-row { flex: 1; display: flex; align-items: center; gap: 12px; text-align: left; background: var(--bg); border: 1px solid var(--line); border-radius: 3px; padding: 12px 15px; cursor: pointer; font: inherit; color: var(--ink); box-shadow: var(--shadow-sm); transition: border-color .14s, box-shadow .14s, transform .14s; min-width: 0; }
  .job-row:hover { border-color: var(--accent); background: color-mix(in srgb, var(--accent) 3%, var(--bg)); }
  .job-badge { font-size: 10.5px; font-weight: 600; letter-spacing: .04em; text-transform: uppercase; padding: 3px 8px; border-radius: 3px; white-space: nowrap; color: #fff; background: var(--faint); }
  .job-badge.transcribe { background: #3a36b0; }
  .job-badge.deidentify { background: #be123c; }
  .job-badge.summarize { background: #0d9488; }
  .job-badge.meeting { background: #7c3aed; }
  .meeting-card { max-width: 560px; margin: 12px auto 0; display: flex; flex-direction: column; gap: 14px; text-align: left; box-sizing: border-box; background: var(--bg); border: 1px solid var(--line); border-radius: 3px; padding: 26px 28px; box-shadow: var(--shadow-md); }
  .consent { display: flex; gap: 10px; align-items: flex-start; padding: 12px 14px; border-radius: 3px; background: color-mix(in srgb, #f59e0b 12%, transparent); border: 1px solid color-mix(in srgb, #f59e0b 30%, transparent); font-size: 13px; line-height: 1.45; color: #7c5410; }
  .consent svg { width: 22px; height: 22px; flex-shrink: 0; color: #b45309; }
  .m-tip { display: flex; gap: 10px; align-items: flex-start; padding: 11px 14px; border-radius: 3px; background: color-mix(in srgb, var(--accent) 6%, var(--bg)); border: 1px solid color-mix(in srgb, var(--accent) 22%, var(--bg)); font-size: 13px; line-height: 1.45; color: var(--ink); }
  .m-tip svg { width: 21px; height: 21px; flex-shrink: 0; color: var(--accent); margin-top: 1px; }
  .m-tip em { font-style: normal; font-weight: 600; }
  .m-fields { display: flex; gap: 12px; }
  .m-field { flex: 1; display: flex; flex-direction: column; gap: 5px; font-size: 12.5px; font-weight: 600; color: #5b6270; }
  .m-field .profile { width: 100%; }
  .big-rec { font-size: 17px; font-weight: 600; display: flex; align-items: center; gap: 10px; justify-content: center; padding: 16px 0 2px; }
  .live-feed { max-height: 340px; overflow-y: auto; text-align: left; background: color-mix(in srgb, var(--accent) 4%, #fff); border: 1px solid color-mix(in srgb, var(--accent) 12%, transparent); border-radius: 3px; padding: 12px 14px; display: flex; flex-direction: column; gap: 7px; margin-top: 4px; }
  .live-line { margin: 0; font-size: 14px; line-height: 1.45; }
  .live-who { display: inline-block; font-size: 10px; font-weight: 700; letter-spacing: .03em; padding: 1px 7px; border-radius: 3px; color: #fff; margin-right: 6px; }
  .live-who.me { background: #3a36b0; }
  .live-who.them { background: #7c3aed; }
  /* Live meeting split view (live-text + notes/actions side by side) */
  .meeting-card.wide { max-width: 100%; background: transparent; border: none; box-shadow: none; padding: 0; }
  .m-live-head { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; justify-content: space-between; }
  .m-live-toggles { display: flex; gap: 6px; }
  .m-split { display: flex; gap: 14px; align-items: stretch; margin-top: 8px; }
  .m-split.single { display: block; }
  .m-pane { flex: 1 1 0; min-width: 0; display: flex; flex-direction: column; border: 1px solid var(--line); border-radius: 3px; padding: 12px 14px; background: #fff; }
  .m-pane-head { margin-bottom: 8px; }
  .m-pane-head h3 { margin: 0; font-size: 12px; font-weight: 700; letter-spacing: .04em; text-transform: uppercase; color: var(--muted); }
  .m-pane-live .live-feed { max-height: 420px; border: none; background: transparent; padding: 0; margin: 0; }
  .m-notes { width: 100%; box-sizing: border-box; min-height: 240px; flex: 1; resize: vertical; font: inherit; font-size: 15px; line-height: 1.6; color: var(--ink); border: 1px solid var(--line-2); border-radius: 3px; padding: 10px 12px; }
  .m-notes:focus { outline: none; border-color: var(--accent); }
  .m-add { margin-top: 10px; }
  .m-actions { list-style: none; margin: 10px 0 0; padding: 0; display: flex; flex-direction: column; gap: 5px; }
  .m-actions li { display: flex; align-items: center; gap: 8px; font-size: 14px; }
  .m-actions li span { flex: 1; min-width: 0; }
  .m-actions li.done span { text-decoration: line-through; color: var(--muted); }
  .m-actions input[type="checkbox"] { width: 15px; height: 15px; accent-color: var(--accent); flex: none; cursor: pointer; }
  .qa { display: flex; flex-direction: column; gap: 14px; max-width: 760px; }
  .qa-history { display: flex; flex-direction: column; gap: 16px; }
  .qa-item { display: flex; flex-direction: column; gap: 6px; }
  .qa-q { margin: 0; font-weight: 600; font-size: 15px; color: var(--ink); }
  .qa-q::before { content: "Du: "; color: var(--accent); }
  .qa-a { margin: 0; font-size: 15px; line-height: 1.55; white-space: pre-wrap; background: var(--bg); border: 1px solid var(--line); border-radius: 3px; padding: 13px 15px; box-shadow: var(--shadow-sm); }
  .qa-empty { margin: 0; }
  .qa-form { display: flex; gap: 8px; position: sticky; bottom: 0; background: var(--bg); padding: 6px 0; }
  .qa-input { flex: 1; padding: 11px 14px; border: 1px solid var(--line-2); border-radius: 3px; font: inherit; }
  .qa-input:focus { outline: none; border-color: var(--accent); }

  /* Meeting workspace (B): notes / participants / actions / follow-up */
  .ws { flex: 1; min-height: 0; overflow-y: auto; overflow-x: hidden; display: flex; flex-direction: column; gap: 16px; width: 100%; max-width: 1160px; margin: 0 auto; padding-bottom: 8px; container-type: inline-size; }
  .ws-grid { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 300px); gap: 16px; align-items: start; }
  .ws-col-main, .ws-col-side { display: flex; flex-direction: column; gap: 16px; min-width: 0; }
  /* Reflow on the actual content width (the app sidebar eats ~360px), not the viewport. */
  @container (max-width: 720px) { .ws-grid { grid-template-columns: 1fr; } }
  /* Viewport fallback for engines without container-query support. */
  @media (max-width: 1080px) { .ws-grid { grid-template-columns: 1fr; } }
  .ws-ico { width: 17px; height: 17px; color: var(--accent); flex: none; }
  .ws-card { border: 1px solid var(--line); border-radius: 3px; background: #fff; padding: 16px 18px; min-width: 0; }
  .ws-max { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .ws-scroll { flex: 1; overflow: auto; }
  .ws-head { display: flex; align-items: center; justify-content: space-between; gap: 10px; margin-bottom: 12px; }
  .ws-head h3 { margin: 0; font-size: 15px; font-weight: 600; color: var(--ink); display: flex; align-items: center; gap: 8px; }
  .ws-head-actions { display: flex; gap: 6px; flex-wrap: wrap; }
  .ws-badge { font-size: 12px; font-weight: 600; color: var(--accent); background: color-mix(in srgb, var(--accent) 8%, var(--bg)); border-radius: 3px; padding: 1px 9px; }
  .ws-people { display: flex; flex-direction: column; gap: 8px; }
  .ws-person { display: flex; flex-direction: column; gap: 6px; padding: 9px 10px; border: 1px solid var(--line); border-radius: 3px; background: #fcfcfd; }
  .ws-person-top { display: flex; gap: 6px; align-items: center; }
  .ws-pname { flex: 1; font-weight: 500; }
  .ws-person input { width: 100%; box-sizing: border-box; min-width: 0; padding: 8px 11px; border: 1px solid var(--line-2); border-radius: 3px; font: inherit; background: #fff; }
  .ws-person input:focus { outline: none; border-color: var(--accent); }
  .ws-notes { width: 100%; box-sizing: border-box; min-width: 0; min-height: 200px; resize: vertical; font: inherit; font-size: 15px; line-height: 1.6; color: var(--ink); border: 1px solid var(--line-2); border-radius: 3px; padding: 12px 14px; }
  .ws-notes:focus { outline: none; border-color: var(--accent); }
  .ws-notes.max { min-height: 0; flex: 1; resize: none; }
  .ws-actions { list-style: none; margin: 0 0 12px; padding: 0; display: flex; flex-direction: column; gap: 8px; }
  .ws-action { display: flex; align-items: flex-start; gap: 10px; padding: 8px 10px; border: 1px solid var(--line); border-radius: 3px; background: #fcfcfd; }
  .ws-action.done { background: #f5f7f5; }
  .ws-action.done .ws-atext { text-decoration: line-through; color: var(--muted); }
  .ws-check { margin-top: 9px; width: 16px; height: 16px; accent-color: var(--accent); flex: none; cursor: pointer; }
  .ws-action-main { flex: 1; display: flex; flex-direction: column; gap: 6px; min-width: 0; }
  .ws-atext { width: 100%; box-sizing: border-box; padding: 7px 10px; border: 1px solid transparent; border-radius: 3px; font: inherit; font-size: 15px; background: transparent; }
  .ws-atext:hover { border-color: var(--line-2); }
  .ws-atext:focus { outline: none; border-color: var(--accent); background: #fff; }
  .ws-ameta { display: flex; gap: 8px; flex-wrap: wrap; }
  .ws-assignee, .ws-due { min-width: 0; padding: 5px 9px; border: 1px solid var(--line-2); border-radius: 3px; font: inherit; font-size: 13px; color: var(--muted); }
  .ws-assignee { width: 160px; max-width: 45%; }
  .ws-due { max-width: 100%; }
  .ws-assignee:focus, .ws-due:focus { outline: none; border-color: var(--accent); color: var(--ink); }
  .ws-action-ctrls { display: flex; align-items: center; gap: 2px; flex: none; }
  .mini { border: none; background: none; cursor: pointer; color: var(--muted); font-size: 14px; width: 24px; height: 24px; border-radius: 3px; line-height: 1; }
  .mini:hover:not(:disabled) { background: color-mix(in srgb, var(--accent) 8%, var(--bg)); color: var(--accent); }
  .mini:disabled { opacity: .3; cursor: default; }
  .ws-add { display: flex; gap: 8px; }
  .ws-add input { flex: 1; min-width: 0; padding: 9px 12px; border: 1px solid var(--line-2); border-radius: 3px; font: inherit; }
  .ws-add input:focus { outline: none; border-color: var(--accent); }
  .ws-paste { display: flex; flex-direction: column; gap: 8px; margin-bottom: 12px; }
  .ws-paste textarea { width: 100%; box-sizing: border-box; resize: vertical; font: inherit; font-size: 14px; border: 1px solid var(--line-2); border-radius: 3px; padding: 10px 12px; }
  .ws-paste textarea:focus { outline: none; border-color: var(--accent); }
  .ws-followup { display: flex; align-items: center; gap: 12px; }
  .ws-followup label { display: flex; flex-direction: column; gap: 5px; font-size: 13px; font-weight: 600; color: var(--muted); }
  .ws-followup input { padding: 8px 11px; border: 1px solid var(--line-2); border-radius: 3px; font: inherit; font-weight: 400; color: var(--ink); width: max-content; max-width: 100%; box-sizing: border-box; }
  .ws-followup input:focus { outline: none; border-color: var(--accent); }
  .maskword { display: inline; cursor: pointer; border: none; background: none; font: inherit; color: inherit; padding: 0 1px; border-radius: 3px; }
  .maskword:hover { background: #fff3cd; box-shadow: 0 0 0 1px #f59e0b; }
  .hit.manual:not(.rejected) { text-decoration: underline; text-decoration-thickness: 2px; text-underline-offset: 2px; }
  .modal-backdrop { position: fixed; inset: 0; background: rgba(17,18,20,.45); display: flex; align-items: center; justify-content: center; z-index: 50; }
  .modal { background: #fff; border-radius: 3px; padding: 22px 24px; width: min(420px, 90vw); box-shadow: 0 20px 60px rgba(0,0,0,.25); display: flex; flex-direction: column; gap: 14px; }
  .modal-title { margin: 0; font-size: 17px; font-weight: 600; }
  .modal-field { display: flex; flex-direction: column; gap: 5px; font-size: 13px; font-weight: 600; color: var(--muted); }
  .modal-field select, .modal-field input { padding: 9px 11px; border: 1px solid var(--line-2); border-radius: 3px; font: inherit; font-weight: 400; color: var(--ink); }
  .modal-field select:focus, .modal-field input:focus { outline: none; border-color: var(--accent); }
  .modal-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 4px; }
  .modal-field textarea { padding: 9px 11px; border: 1px solid var(--line-2); border-radius: 3px; font: inherit; font-weight: 400; color: var(--ink); resize: vertical; }
  .modal-field textarea:focus { outline: none; border-color: var(--accent); }
  .modal.ai-modal { width: min(560px, 92vw); }
  .ai-status.ok { margin: 0; font-size: 13px; color: #047857; font-weight: 500; }
  .ai-confirm { display: flex; align-items: center; gap: 8px; font-size: 13px; color: #92400e; font-weight: 500; }
  .ai-prompt-actions { display: flex; gap: 16px; margin-top: -6px; }
  .ai-open { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; padding-top: 12px; border-top: 1px solid var(--line-2); }
  .ai-open-label { font-size: 13px; font-weight: 600; color: var(--muted); margin-right: 2px; }
  .btn.small { padding: 6px 10px; font-size: 13px; }
  .working { display: inline-flex; align-items: center; gap: 8px; align-self: flex-start; background: color-mix(in srgb, var(--accent) 8%, var(--bg)); color: var(--accent); border: 1px solid color-mix(in srgb, var(--accent) 18%, var(--bg)); border-radius: 3px; padding: 5px 13px; font-size: 12.5px; font-weight: 600; margin-bottom: 12px; }
  .working-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); animation: workpulse 1s ease-in-out infinite; }
  .rev-speaker { font-weight: 600; color: var(--muted); }
  .t-toolbar { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; margin-bottom: 12px; }
  .t-toolbar .meta { margin: 0; }
  .btn.on { background: var(--accent); color: #fff; border-color: var(--accent); }
  .job-search { width: 100%; max-width: 520px; padding: 9px 13px; border: 1px solid var(--line-2); border-radius: 3px; font: inherit; margin-bottom: 18px; display: block; }
  .job-search:focus { outline: none; border-color: var(--accent); }
  .job-path { color: var(--faint); font-weight: 400; }
  .job-item { flex-wrap: wrap; }
  .job-item.dragging { opacity: .45; }
  .job-cat-wrap { position: relative; }
  .job-cat-btn { display: inline-flex; align-items: center; gap: 6px; max-width: 180px; padding: 6px 9px; border: 1px solid var(--line-2); border-radius: 3px; background: #fff; font: inherit; font-size: 12.5px; color: var(--muted); cursor: pointer; }
  .job-cat-btn svg { width: 14px; height: 14px; color: var(--accent); flex-shrink: 0; }
  .job-cat-btn span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .job-cat-btn:hover { border-color: var(--accent); color: var(--ink); }
  .job-menu { border: none; background: none; color: var(--muted); cursor: pointer; font-size: 18px; line-height: 1; padding: 0 8px; border-radius: 3px; }
  .job-menu:hover { background: color-mix(in srgb, var(--accent) 8%, var(--bg)); color: var(--accent); }
  .drag-handle { cursor: grab; color: var(--faint); font-size: 13px; padding: 0 4px; user-select: none; flex-shrink: 0; line-height: 1; }
  .drag-handle:hover { color: var(--accent); }
  .drag-handle:active { cursor: grabbing; }
  .app.grabbing { user-select: none; cursor: grabbing; }
  .drag-ghost { position: fixed; z-index: 100; pointer-events: none; transform: translate(14px, 10px); background: var(--accent); color: #fff; padding: 6px 12px; border-radius: 3px; font-size: 13px; font-weight: 500; max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; box-shadow: 0 10px 30px rgba(58,54,176,.4); }
  .job-check { width: 15px; height: 15px; accent-color: var(--accent); flex-shrink: 0; cursor: pointer; }
  .job-item.sel { background: color-mix(in srgb, var(--accent) 8%, var(--bg)); border-radius: 3px; }
  .bulkbar { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; background: color-mix(in srgb, var(--accent) 8%, var(--bg)); border: 1px solid color-mix(in srgb, var(--accent) 18%, var(--bg)); border-radius: 3px; padding: 8px 12px; margin-bottom: 12px; }
  .bulk-n { font-weight: 600; font-size: 13px; color: var(--accent); margin-right: 4px; }
  .bulk-move { position: relative; }
  .job-cat-wrap .fp-menu { left: auto; right: 0; }
  .hist { display: flex; gap: 18px; align-items: flex-start; padding:24px; box-sizing:border-box; min-width:0; }
  .hist-tree { flex: 0 0 240px; display: flex; flex-direction: column; gap: 2px; max-height: calc(100vh - 130px); overflow: auto; padding-right: 4px; }
  .tree-rowwrap { display: flex; align-items: center; border-radius: 3px; }
  .tree-rowwrap.drop, .tree-node.drop { outline: 2px solid var(--accent); outline-offset: -2px; background: color-mix(in srgb, var(--accent) 8%, var(--bg)); }
  .tree-twirl { border: none; background: none; cursor: pointer; color: var(--faint); width: 18px; flex-shrink: 0; font-size: 11px; transition: transform .12s; }
  .tree-twirl.open { transform: rotate(90deg); }
  .tree-twirl-spacer { width: 18px; flex-shrink: 0; display: inline-block; }
  .tree-node { flex: 1; display: flex; align-items: center; gap: 8px; min-width: 0; text-align: left; background: none; border: none; border-radius: 3px; padding: 7px 9px; font: inherit; font-size: 13.5px; color: var(--ink); cursor: pointer; }
  .tree-node:hover { background: #f4f6ff; }
  .tree-node.on { background: color-mix(in srgb, var(--accent) 8%, var(--bg)); font-weight: 600; }
  .tree-ico { width: 15px; height: 15px; color: var(--accent); flex-shrink: 0; }
  .tree-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tree-count { color: var(--faint); font-size: 12px; }
  .tree-rename { flex: 1; min-width: 0; padding: 6px 8px; border: 1px solid var(--accent); border-radius: 3px; font: inherit; font-size: 13.5px; }
  .tree-rename:focus { outline: none; }
  .tree-new { text-align: left; border: 1px dashed var(--line-2); background: none; color: var(--muted); cursor: pointer; font: inherit; font-size: 13px; padding: 7px 10px; border-radius: 3px; margin-top: 6px; }
  .tree-new:hover { border-color: var(--accent); color: var(--accent); }
  .hist-main { flex: 1; min-width: 0; }
  .library-tools { display:flex;align-items:end;gap:18px;padding:20px 28px 8px;flex-wrap:wrap; }
  .library-search { flex:1;min-width:min(240px,100%);font-size:13px;color:var(--muted); }
  .library-search .job-search { box-sizing:border-box;display:block;width:100%;margin:6px 0 0; }
  .library-status { margin:4px 28px 12px;font-size:14px;color:var(--muted); }
  .hist-bar { display: flex; align-items: baseline; gap: 10px; margin: 4px 0 12px; }
  .hist-where { font-size: 16px; font-weight: 600; }
  .hist-count { color: var(--faint); font-size: 13px; }
  .ctx { position: fixed; z-index: 61; min-width: 180px; background: #fff; border: 1px solid var(--line-2); border-radius: 3px; box-shadow: 0 16px 44px rgba(0,0,0,.18); padding: 5px; display: flex; flex-direction: column; gap: 1px; }
  .ctx-item { text-align: left; background: none; border: none; border-radius: 3px; padding: 8px 11px; font: inherit; font-size: 13.5px; color: var(--ink); cursor: pointer; }
  .ctx-item:hover { background: color-mix(in srgb, var(--accent) 8%, var(--bg)); }
  .ctx-item.danger { color: #c0392b; }
  .ctx-item.danger:hover { background: #fde8e8; }
  .job-rename { flex: 1; min-width: 0; padding: 10px 12px; border: 1px solid var(--accent); border-radius: 4px; font: inherit; color: var(--ink); }
  .job-rename:focus { outline: none; }
  .hdr-folder { position: relative; margin-right: 12px; }
  .hdr-folder-btn { display: inline-flex; align-items: center; gap: 7px; border: 1px solid var(--line-2); border-radius: 3px; padding: 6px 11px; background: #fff; font: inherit; font-size: 13px; color: var(--muted); cursor: pointer; max-width: 240px; }
  .hdr-folder-btn:hover { border-color: var(--accent); }
  .hdr-folder-btn svg { width: 15px; height: 15px; color: var(--accent); flex-shrink: 0; }
  .hdr-folder-name { color: var(--ink); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hdr-folder-btn .chev { width: 13px; height: 13px; color: var(--faint); }
  .fp-backdrop { position: fixed; inset: 0; z-index: 60; }
  .fp-menu { position: absolute; top: calc(100% + 6px); left: 0; z-index: 61; min-width: 240px; max-width: 320px; max-height: 360px; overflow: auto; background: #fff; border: 1px solid var(--line-2); border-radius: 3px; box-shadow: 0 16px 44px rgba(0,0,0,.18); padding: 6px; display: flex; flex-direction: column; gap: 1px; }
  .hdr-folder .fp-menu { left: auto; right: 0; }
  .fp-row { display: flex; align-items: center; }
  .fp-item { flex: 1; display: flex; align-items: center; gap: 8px; text-align: left; background: none; border: none; border-radius: 3px; padding: 7px 9px; font: inherit; font-size: 13.5px; color: var(--ink); cursor: pointer; min-width: 0; }
  .fp-item:hover { background: color-mix(in srgb, var(--accent) 8%, var(--bg)); }
  .fp-item.on { background: color-mix(in srgb, var(--accent) 8%, var(--bg)); font-weight: 600; }
  .fp-item svg { width: 15px; height: 15px; color: var(--accent); flex-shrink: 0; }
  .fp-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fp-count { color: var(--faint); font-size: 12px; }
  .fp-sub { border: none; background: none; color: var(--faint); cursor: pointer; font-size: 16px; line-height: 1; padding: 2px 8px; border-radius: 3px; }
  .fp-sub:hover { background: color-mix(in srgb, var(--accent) 8%, var(--bg)); color: var(--accent); }
  .fp-create { display: flex; gap: 6px; padding: 8px 6px 4px; border-top: 1px solid var(--line); margin-top: 4px; }
  .fp-create input { flex: 1; min-width: 0; padding: 7px 9px; border: 1px solid var(--line-2); border-radius: 3px; font: inherit; font-size: 13px; }
  .fp-create input:focus { outline: none; border-color: var(--accent); }
  @keyframes workpulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.25; } }
  .sel-mask-btn { position: fixed; transform: translate(-50%, -125%); z-index: 45; background: var(--accent); color: #fff; border: none; border-radius: 3px; padding: 6px 12px; font: inherit; font-size: 13px; font-weight: 600; cursor: pointer; box-shadow: 0 6px 18px rgba(58,54,176,.35); white-space: nowrap; }
  .sel-mask-btn:hover { filter: brightness(1.08); }
  .empty-view { max-width: 460px; margin: 48px auto; text-align: center; }
  .empty-view h3 { font-family: "Instrument Serif", serif; font-weight: 400; margin: 0 0 8px; font-size: 25px; letter-spacing: -.01em; color: var(--ink); }
  .empty-view .hint { font-size: 13.5px; line-height: 1.55; }
  .job-title { flex: 1; font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .job-date { font-size: 12px; color: var(--faint); white-space: nowrap; }
  .job-ws { display: inline-flex; align-items: center; gap: 6px; flex: none; }
  .job-chip { display: inline-flex; align-items: center; gap: 3px; font-size: 11.5px; font-weight: 600; color: var(--muted); background: #f1f2f4; border-radius: 3px; padding: 2px 8px; }
  .job-chip svg { width: 13px; height: 13px; }
  .job-chip.done { color: #0d9488; background: #e7f6f3; }
  .job-chip.pending { color: #b45309; background: #fef3c7; }
  .home-open { display: inline-block; margin-top: 30px; }
  .big-hint { font-size: 14px; line-height: 1.6; max-width: 520px; }

  /* ---- standalone source picker ---- */
  .src-modes { display: flex; flex-direction: column; gap: 6px; margin-bottom: 10px; }
  .radio { display: flex; align-items: center; gap: 8px; font-size: 13.5px; cursor: pointer; }
  .src-text { width: 100%; box-sizing: border-box; font: inherit; font-size: 13.5px; line-height: 1.5; border: 1px solid var(--line-2); border-radius: 4px; padding: 9px 11px; resize: vertical; }
  .src-text:focus { outline: none; border-color: var(--accent); }

  /* ---- Åtaganden (cross-project action overview) ---- */

  .tasks { max-width: 980px; margin: 0 auto; padding: 4px 2px 60px; }
  .tasks-head { margin-bottom: 18px; }
  .tasks-head .big-title { margin-bottom: 6px; }
  .tasks-sub { color: var(--muted); font-size: 13.5px; margin: 0; }
  .t-overdue-text { color: #c0392b; font-weight: 500; }

  .task-add { display: flex; gap: 8px; align-items: center; margin-bottom: 16px; flex-wrap: wrap; }
  .task-add input, .task-add .profile { font: inherit; font-size: 13.5px; border: 1px solid var(--line-2); background: var(--bg); color: var(--ink); border-radius: 3px; padding: 9px 11px; }
  .task-add .ta-text { flex: 1 1 240px; min-width: 200px; }
  .task-add .ta-who { width: 160px; }
  .task-add .ta-due { width: 150px; }
  .task-add .ta-target { width: 180px; cursor: pointer; }
  .task-add input:focus, .task-add .profile:focus { outline: none; border-color: var(--accent); }

  .tasks-bar { display: flex; gap: 8px; align-items: center; flex-wrap: wrap; margin-bottom: 14px; padding-bottom: 14px; border-bottom: 1px solid var(--line); }
  .tasks-bar .task-search { flex: 1 1 200px; min-width: 160px; font: inherit; font-size: 13.5px; border: 1px solid var(--line-2); background: var(--bg); color: var(--ink); border-radius: 3px; padding: 8px 11px; }
  .tasks-bar .task-search:focus { outline: none; border-color: var(--accent); }
  .tasks-bar .profile { font-size: 13px; padding: 8px 26px 8px 10px; }

  .seg { display: inline-flex; border: 1px solid var(--line-2); border-radius: 3px; overflow: hidden; }
  .seg button { font: inherit; font-size: 13px; padding: 8px 13px; border: none; background: var(--bg); color: var(--muted); cursor: pointer; border-right: 1px solid var(--line-2); transition: .12s; }
  .seg button:last-child { border-right: none; }
  .seg button:hover { background: var(--canvas); color: var(--ink); }
  .seg button.on { background: var(--accent); color: #fff; }

  .task-group { font-family: "Instrument Serif", serif; font-weight: 400; font-size: 22px; color: var(--ink); margin: 22px 0 8px; display: flex; align-items: baseline; gap: 9px; }
  .task-group .tg-count { font-family: inherit; font-size: 13px; color: var(--faint); }

  .task-list { list-style: none; margin: 0 0 6px; padding: 0; }
  .task-row { display: flex; align-items: flex-start; gap: 11px; padding: 11px 4px; border-bottom: 1px solid var(--line); }
  .task-row:hover { background: color-mix(in srgb, var(--accent) 3%, transparent); }
  .task-row.overdue { box-shadow: inset 2px 0 0 #c0392b; }
  .t-check { margin-top: 3px; width: 16px; height: 16px; accent-color: var(--accent); flex: none; cursor: pointer; }
  .t-main { flex: 1; min-width: 0; }
  .t-text { width: 100%; box-sizing: border-box; font: inherit; font-size: 14.5px; color: var(--ink); border: 1px solid transparent; background: transparent; border-radius: 3px; padding: 3px 6px; margin: -3px -6px 2px; }
  .t-text:hover { border-color: var(--line); }
  .t-text:focus { outline: none; border-color: var(--accent); background: var(--bg); }
  .task-row.done .t-text { color: var(--faint); text-decoration: line-through; }
  .t-meta { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .t-proj { display: inline-flex; align-items: center; gap: 5px; font: inherit; font-size: 12px; color: var(--accent); background: color-mix(in srgb, var(--accent) 8%, transparent); border: none; border-radius: 3px; padding: 3px 8px; cursor: pointer; max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .t-proj:hover { background: color-mix(in srgb, var(--accent) 16%, transparent); }
  .t-proj svg { width: 13px; height: 13px; flex: none; }
  .t-proj.free { color: var(--muted); background: var(--canvas); border: 1px solid var(--line); cursor: default; }
  .t-who { width: 130px; font: inherit; font-size: 12.5px; color: var(--muted); border: 1px solid transparent; background: transparent; border-radius: 3px; padding: 3px 6px; }
  .t-who:hover { border-color: var(--line); }
  .t-who:focus { outline: none; border-color: var(--accent); background: var(--bg); color: var(--ink); }
  .t-due { font: inherit; font-size: 12.5px; color: var(--muted); border: 1px solid transparent; background: transparent; border-radius: 3px; padding: 3px 6px; }
  .t-due:hover { border-color: var(--line); }
  .t-due:focus { outline: none; border-color: var(--accent); background: var(--bg); color: var(--ink); }
  .t-due-text { font-size: 12px; color: var(--muted); background: var(--canvas); border: 1px solid var(--line); border-radius: 3px; padding: 2px 7px; }
  .t-flag { font-size: 11px; font-weight: 600; color: #fff; background: #c0392b; border-radius: 3px; padding: 2px 7px; }
  .t-del { flex: none; font-size: 20px; line-height: 1; color: var(--faint); background: none; border: none; cursor: pointer; padding: 2px 6px; border-radius: 3px; }
  .t-del:hover { color: #c0392b; background: #fde8e8; }

  .workspace-header { align-items:center; min-height:78px; padding:16px 28px; flex-wrap:wrap; }
  .workspace-heading { display:grid; gap:4px; min-width:0; max-width:55ch; overflow-wrap:anywhere; }
  .workspace-location { color:var(--muted); font-size:13px; } .workspace-heading strong { font-size:16px; font-weight:500; }
  .save-status { display:flex; flex-wrap:wrap; gap:7px; font-size:13px; color:var(--muted); } .save-status span { font-variant-numeric:tabular-nums; } .save-status.failed { color:#923115; }
  .save-failure { background:#fff3ea; border-bottom:1px solid #e8cdbd; padding:16px 28px; display:flex; flex-wrap:wrap; gap:12px; align-items:center; color:#923115; } .save-failure div { flex:1; min-width:180px; } .save-failure p { margin:5px 0 0; overflow-wrap:anywhere; font-size:13px; }
  .workspace-tabs { display:flex; flex-wrap:wrap; gap:6px; padding:14px 28px; border-bottom:1px solid var(--line); }
  .workspace-tabs button { font:inherit; font-size:14px; color:var(--muted); background:transparent; border:0; border-radius:7px; padding:8px 11px; cursor:pointer; } .workspace-tabs button[aria-pressed=true] { background:var(--accent-soft); color:var(--accent); }
  .panel-control { padding:10px 28px; border-bottom:1px solid var(--line); }
  .home-intro { margin:12px 0 0; color:var(--muted); font-size:16px; }
  .entrypoints { display:grid; grid-template-columns:repeat(3,minmax(0,1fr)); border-block:1px solid var(--line-2); margin:30px 0 20px; }
  .entrypoints button { font:inherit; color:var(--ink); background:none; border:0; text-align:left; padding:24px 22px; cursor:pointer; display:flex; flex-direction:column; }
  .entrypoints button:first-child { padding-left:0; } .entrypoints button + button { border-left:1px solid var(--line-2); } .entrypoints button:hover { background:var(--nav-bg); }
  .entrypoints h3 { font:28px/1.2 'Instrument Serif',serif; margin:0 0 12px; } .entrypoints p { font-size:14px; line-height:1.7; color:var(--muted); margin:0 0 18px; } .entrypoints span { font-size:13px; color:var(--accent); margin-top:auto; }
  .home-tools { display:flex; flex-wrap:wrap; gap:22px; margin-bottom:32px; } .meeting-heading { display:flex; flex-wrap:wrap; gap:16px; align-items:center; margin-bottom:24px; } .meeting-heading .big-title { margin:0 auto 0 0; }
  .job-strip .job-row { width:100%; border:0; border-bottom:1px solid var(--line); border-radius:0; padding:18px 0; }
  .job-strip .job-badge { color:var(--accent); background:var(--accent-soft); text-transform:none; font-size:12px; letter-spacing:0; }
  @media(max-width:550px) { .job-strip .job-row { flex-wrap:wrap; gap:8px; } .job-strip .job-title { white-space:normal; overflow-wrap:anywhere; } .job-strip .job-date { flex-basis:100%; } }
  .btn,textarea,select.profile,.document,.summary-edit { border-radius:7px; }
  .review { min-width:0; min-height:0; } .review-head { flex-wrap:wrap; } .document,.summary-edit { min-height:200px; }
  .layout { min-height:440px; } .summary-edit { font-size:16px; } .ts { font-size:12px; }
  :global(button:focus-visible),:global(input:focus-visible),:global(textarea:focus-visible),:global(select:focus-visible),:global(summary:focus-visible) { outline:3px solid var(--accent); outline-offset:3px; }
  @media(max-width:1050px) { .app { height:auto; min-height:100dvh; grid-template-columns:1fr; } .app-content { overflow:visible; } .layout { overflow:visible; } .review { overflow:visible; } }
  @media(max-width:760px) { .layout { grid-template-columns:1fr; } .layout.collapsed { grid-template-columns:1fr; } .layout.collapsed .sidebar { display:none; } .sidebar { border-right:0; border-bottom:1px solid var(--line); } .workspace-header { padding:14px 18px; } .workspace-tabs { padding:12px 18px; } .review { padding:20px 18px; } .entrypoints { grid-template-columns:1fr; } .entrypoints button,.entrypoints button:first-child { padding:20px 0; } .entrypoints button + button { border-left:0; border-top:1px solid var(--line); } .home { padding:26px 20px; } .home .big-title { font-size:36px; } }
  @media(prefers-reduced-motion:reduce) { :global(*),:global(*::before),:global(*::after) { animation:none!important; transition:none!important; scroll-behavior:auto!important; } }

  .dictation-container { min-width:0; overflow:auto; }
  .dictation-container[hidden] { display:none; }
  .saved-dictations { margin-top:28px; }
  @media(max-width:800px) {
    .hist { flex-direction:column; padding:16px; }
    .hist-tree { flex:0 0 auto; width:100%; max-height:180px; box-sizing:border-box; }
    .hist-main { width:100%; }
    .hist .job-row { flex-wrap:wrap; }
    .hist .job-title { flex-basis:55%; }
    .hist .job-date { white-space:normal; }
  }
  .model-reference { display:flex;flex-wrap:wrap;align-items:center;gap:6px 12px;font-size:13px;line-height:1.6;margin:8px 0;color:var(--muted); }
  .model-reference strong { color:var(--ink);font-weight:500; }.model-reference small { flex-basis:100%; }.secondary-controls { margin-top:20px; }.secondary-controls summary { cursor:pointer;font-size:13px;color:var(--muted);margin-bottom:14px; }
  :global(button:focus-visible), :global(select:focus-visible), :global(input:focus-visible), :global(summary:focus-visible) { outline:2px solid var(--accent);outline-offset:3px; }
</style>
