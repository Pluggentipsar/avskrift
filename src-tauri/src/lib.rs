//! Tauri command surface for Avskrift: download/list models, transcribe (+diarise), anonymise the
//! transcript, and export. Heavy work runs on a blocking thread pool via `tauri::async_runtime`.

mod ai;
mod align;
mod audio;
mod diarize;
mod docio;
mod download;
mod engine;
mod models;
mod pii;
mod summarize;
mod transcribe;
mod transcript;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use engine::{AnalyzeResult, Engine, ModelPaths as PiiPaths};
use models::{ModelPaths, SummaryModelInfo, WhisperModelInfo};
use pii::Category;
use summarize::Summarizer;
use tauri::{AppHandle, Emitter, Manager, State};
use transcribe::Transcriber;
use transcript::Transcript;

/// All long-lived backend state, managed by Tauri.
struct Backend {
    engine: Engine,
    transcriber: Mutex<Transcriber>,
    /// The most recent transcript (timings + speakers), held for export.
    transcript: Mutex<Option<Transcript>>,
    /// Lazily-loaded summariser, keyed by the loaded model id (reloaded on change).
    summarizer: Mutex<Option<(String, Summarizer)>>,
    paths: ModelPaths,
}

fn emit(app: &AppHandle, msg: impl Into<String>) {
    let _ = app.emit("avskrift:progress", msg.into());
}

// ---- Models ----

#[tauri::command]
fn list_whisper_models(backend: State<Backend>) -> Vec<WhisperModelInfo> {
    backend.paths.whisper_catalogue()
}

/// Download a Whisper model by id into the writable model dir, emitting `avskrift:download`
/// progress events with `{ id, downloaded, total }`.
#[tauri::command]
async fn download_whisper_model(app: AppHandle, id: String) -> Result<(), String> {
    let (url, dest) = {
        let backend = app.state::<Backend>();
        let url = models::whisper_url(&id)
            .ok_or_else(|| format!("okänd modell: {id}"))?
            .to_string();
        (url, backend.paths.whisper_file(&id))
    };

    let app_for_cb = app.clone();
    let id_cb = id.clone();
    tauri::async_runtime::spawn_blocking(move || {
        download::to_file(&url, &dest, &|downloaded, total| {
            let _ = app_for_cb.emit(
                "avskrift:download",
                serde_json::json!({ "id": id_cb, "downloaded": downloaded, "total": total }),
            );
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

// ---- Summarisation models & templates ----

#[tauri::command]
fn list_summary_models(backend: State<Backend>) -> Vec<SummaryModelInfo> {
    backend.paths.summary_catalogue()
}

#[derive(serde::Serialize)]
struct TemplateInfo {
    id: String,
    label: String,
}

#[tauri::command]
fn list_summary_templates() -> Vec<TemplateInfo> {
    summarize::TEMPLATES
        .iter()
        .map(|t| TemplateInfo { id: t.id.to_string(), label: t.label.to_string() })
        .collect()
}

/// Download a summary model (GGUF + tokenizer) by id, emitting `avskrift:download` progress.
#[tauri::command]
async fn download_summary_model(app: AppHandle, id: String) -> Result<(), String> {
    let (gguf_url, tok_url, gguf_dest, tok_dest) = {
        let backend = app.state::<Backend>();
        let (gguf_url, tok_url) =
            models::summary_urls(&id).ok_or_else(|| format!("okänd modell: {id}"))?;
        let (gguf_dest, tok_dest) = backend.paths.summary_files(&id);
        (gguf_url.to_string(), tok_url.to_string(), gguf_dest, tok_dest)
    };

    let app_cb = app.clone();
    let id_cb = id.clone();
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        // Tokenizer first (small), then the large GGUF with progress.
        download::to_file(&tok_url, &tok_dest, &|_, _| {}).map_err(|e| e.to_string())?;
        download::to_file(&gguf_url, &gguf_dest, &|downloaded, total| {
            let _ = app_cb.emit(
                "avskrift:download",
                serde_json::json!({ "id": id_cb, "downloaded": downloaded, "total": total }),
            );
        })
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummarizeArgs {
    /// Transcript text to summarise (utterances joined; may be the anonymised version).
    text: String,
    model: String,
    /// Built-in template id, or "custom" to use `custom_headings`.
    template: String,
    /// User's own agenda/headings, used when `template == "custom"`.
    #[serde(default)]
    custom_headings: String,
}

/// Generate structured Swedish minutes from a transcript. Returns the draft markdown — always shown
/// to the user as an editable draft with an "AI-generated, review" warning.
#[tauri::command]
async fn summarize(app: AppHandle, args: SummarizeArgs) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || run_summarize(&app, args))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn run_summarize(app: &AppHandle, args: SummarizeArgs) -> anyhow::Result<String> {
    let backend = app.state::<Backend>();
    let progress = |m: &str| emit(app, m);

    let (gguf, tok) = backend.paths.summary_files(&args.model);
    if !gguf.exists() || !tok.exists() {
        return Err(anyhow::anyhow!(
            "Sammanfattningsmodellen '{}' är inte nedladdad. Hämta den först.",
            args.model
        ));
    }

    // Lazily (re)load the summariser when the selected model changes.
    let mut guard = backend.summarizer.lock().unwrap();
    let needs_load = !matches!(&*guard, Some((cur, _)) if *cur == args.model);
    if needs_load {
        progress(&format!("Laddar modell ({})…", args.model));
        *guard = Some((args.model.clone(), Summarizer::load(&gguf, &tok)?));
    }
    // Resolve the structure instruction: built-in template, or the user's own agenda.
    let structure: String = if args.template == "custom" {
        if args.custom_headings.trim().is_empty() {
            return Err(anyhow::anyhow!("ingen egen mall angiven"));
        }
        summarize::custom_structure(&args.custom_headings)
    } else {
        summarize::template(&args.template)
            .ok_or_else(|| anyhow::anyhow!("okänd mall: {}", args.template))?
            .structure
            .to_string()
    };

    let (_, summarizer) = guard.as_ref().unwrap();
    summarizer.summarize(&args.text, &structure, &progress)
}

// ---- Transcription ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TranscribeArgs {
    path: String,
    model: String,
    /// ISO code or "auto".
    language: String,
    diarize: bool,
    /// Force a speaker count, or `None` to let clustering decide.
    num_speakers: Option<usize>,
    /// Capture word-level timestamps (slightly slower).
    #[serde(default)]
    word_timestamps: bool,
    /// Translate speech to English instead of transcribing verbatim.
    #[serde(default)]
    translate: bool,
}

#[tauri::command]
async fn transcribe(app: AppHandle, args: TranscribeArgs) -> Result<Transcript, String> {
    tauri::async_runtime::spawn_blocking(move || run_transcription(&app, args))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn run_transcription(app: &AppHandle, args: TranscribeArgs) -> anyhow::Result<Transcript> {
    let backend = app.state::<Backend>();
    let progress = |m: &str| emit(app, m);

    progress("Läser ljudfil…");
    let audio = audio::load(Path::new(&args.path))?;

    let model_path = backend.paths.whisper_file(&args.model);
    let app_pct = app.clone();
    let raw = {
        let mut tr = backend.transcriber.lock().unwrap();
        tr.transcribe(
            &args.model,
            &model_path,
            &audio.samples,
            &args.language,
            args.word_timestamps,
            args.translate,
            &progress,
            move |p| {
                let _ = app_pct.emit("avskrift:percent", p);
            },
        )?
    };

    let utterances = if args.diarize {
        let turns = diarize::diarize(
            &backend.paths.diar_segmentation,
            &backend.paths.diar_embedding,
            &audio.samples,
            args.num_speakers,
            &progress,
        )?;
        align::with_speakers(raw, &turns)
    } else {
        align::without_speakers(raw)
    };

    let transcript = Transcript {
        utterances,
        language: args.language.clone(),
        model: args.model.clone(),
        diarized: args.diarize,
    };
    *backend.transcript.lock().unwrap() = Some(transcript.clone());
    progress("Klar");
    Ok(transcript)
}

// ---- Recording ----

/// Persist a recording captured in the webview (16-bit PCM WAV bytes) to a temp file and return its
/// path, so the existing `transcribe` pipeline can pick it up like any other audio file.
#[tauri::command]
fn save_recording(data: Vec<u8>) -> Result<String, String> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("avskrift-inspelning-{ts}.wav"));
    std::fs::write(&path, &data).map_err(|e| format!("kunde inte spara inspelningen: {e}"))?;
    Ok(path.to_string_lossy().to_string())
}

// ---- Editing & projects ----

/// Replace the stored transcript with an edited copy (e.g. after the user fixed ASR errors or
/// renamed speakers). All downstream steps — anonymisation, summarisation, export — use this.
#[tauri::command]
fn update_transcript(backend: State<Backend>, transcript: Transcript) {
    *backend.transcript.lock().unwrap() = Some(transcript);
}

/// A saved project: the transcript plus the UI's speaker-label overrides. Audio is referenced by
/// path, not embedded, to keep project files small.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Project {
    version: u32,
    transcript: Transcript,
    #[serde(default)]
    speaker_labels: BTreeMap<String, String>,
    #[serde(default)]
    audio_path: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveProjectArgs {
    path: String,
    speaker_labels: BTreeMap<String, String>,
    audio_path: Option<String>,
}

/// Save the current transcript + labels to a `.avskrift` JSON project file.
#[tauri::command]
fn save_project(backend: State<Backend>, args: SaveProjectArgs) -> Result<(), String> {
    let guard = backend.transcript.lock().unwrap();
    let transcript = guard
        .as_ref()
        .ok_or_else(|| "det finns inget transkript att spara".to_string())?
        .clone();
    let project = Project {
        version: 1,
        transcript,
        speaker_labels: args.speaker_labels,
        audio_path: args.audio_path,
    };
    let json = serde_json::to_string_pretty(&project).map_err(|e| e.to_string())?;
    std::fs::write(&args.path, json).map_err(|e| format!("kunde inte spara projektet: {e}"))
}

/// Open a `.avskrift` project file; loads its transcript into backend state and returns it (with
/// labels and audio path) so the UI can restore the session.
#[tauri::command]
fn open_project(backend: State<Backend>, path: String) -> Result<Project, String> {
    let json = std::fs::read_to_string(&path).map_err(|e| format!("kunde inte läsa projektet: {e}"))?;
    let project: Project = serde_json::from_str(&json).map_err(|e| format!("ogiltig projektfil: {e}"))?;
    *backend.transcript.lock().unwrap() = Some(project.transcript.clone());
    Ok(project)
}

// ---- Anonymisation (reuses the Avidentifierare engine over the transcript) ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnonArgs {
    /// Utterance texts in order (possibly edited in the UI).
    texts: Vec<String>,
    enabled: Vec<Category>,
    terms: Vec<String>,
    use_ai: bool,
}

#[tauri::command]
async fn anonymize(app: AppHandle, args: AnonArgs) -> Result<AnalyzeResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let backend = app.state::<Backend>();
        let progress = |m: &str| emit(&app, m);
        backend
            .engine
            .analyze_segments(args.texts, args.enabled, args.terms, args.use_ai, &progress)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Masked text per utterance for the last anonymisation (for in-UI preview / copy).
#[tauri::command]
fn anonymized_segments(backend: State<Backend>, rejected: Vec<usize>) -> Result<Vec<String>, String> {
    backend.engine.anonymized_segments(rejected).map_err(|e| e.to_string())
}

// ---- Export ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportArgs {
    path: String,
    /// Apply anonymisation (uses the last `anonymize` result), or export the raw transcript.
    anonymize: bool,
    /// Span ids the reviewer turned off (only relevant when `anonymize`).
    rejected: Vec<usize>,
    /// Speaker id -> display name overrides.
    speaker_labels: BTreeMap<String, String>,
    /// For VTT only: emit one cue per word (requires a word-timestamped transcript). Raw text only.
    #[serde(default)]
    word_level: bool,
    /// Prefix each utterance with a `[mm:ss]` timestamp in txt/docx export.
    #[serde(default)]
    timestamps: bool,
}

#[tauri::command]
fn export_transcript(backend: State<Backend>, args: ExportArgs) -> Result<(), String> {
    export_inner(&backend, args).map_err(|e| e.to_string())
}

fn export_inner(backend: &Backend, args: ExportArgs) -> anyhow::Result<()> {
    let guard = backend.transcript.lock().unwrap();
    let transcript = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("det finns inget transkript att exportera"))?;

    // When anonymising, fetch masked text per utterance so timestamps & speakers are preserved.
    let masked: Option<Vec<String>> =
        if args.anonymize { Some(backend.engine.anonymized_segments(args.rejected)?) } else { None };
    let texts = masked.as_deref();
    let labels = &args.speaker_labels;
    let out = PathBuf::from(&args.path);

    match ext(&out).as_deref() {
        Some("srt") => docio::save_text(&out, &transcript.to_srt(texts, labels))?,
        Some("vtt") if args.word_level && !args.anonymize => {
            docio::save_text(&out, &transcript.to_vtt_words(labels))?
        }
        Some("vtt") => docio::save_text(&out, &transcript.to_vtt(texts, labels))?,
        Some("docx") if args.timestamps => {
            docio::save_docx(&out, &transcript.to_docx_paragraphs_timed(texts, labels))?
        }
        Some("docx") => docio::save_docx(&out, &transcript.to_docx_paragraphs(texts, labels))?,
        _ if args.timestamps => docio::save_text(&out, &transcript.to_text_timed(texts, labels))?,
        _ => docio::save_text(&out, &transcript.to_text(texts, labels))?, // txt / default
    }
    Ok(())
}

fn ext(p: &Path) -> Option<String> {
    p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveSummaryArgs {
    path: String,
    /// The (edited) summary draft.
    text: String,
    /// Also append the full transcript below the summary (combined document).
    #[serde(default)]
    include_transcript: bool,
    /// Timestamps on the appended transcript.
    #[serde(default)]
    timestamps: bool,
    #[serde(default)]
    speaker_labels: BTreeMap<String, String>,
}

/// Save an (edited) summary draft as plain text or .docx, optionally with the full transcript
/// appended (combined "protokoll + transkript" document). Markdown is written as-is in txt; in docx
/// each line becomes a paragraph (headings kept as literal text in v1).
#[tauri::command]
fn save_summary(backend: State<Backend>, args: SaveSummaryArgs) -> Result<(), String> {
    let mut text = args.text;
    if args.include_transcript {
        let guard = backend.transcript.lock().unwrap();
        if let Some(t) = guard.as_ref() {
            let body = if args.timestamps {
                t.to_text_timed(None, &args.speaker_labels)
            } else {
                t.to_text(None, &args.speaker_labels)
            };
            text.push_str("\n\n## Transkript\n\n");
            text.push_str(&body);
        }
    }
    let out = PathBuf::from(&args.path);
    let res = match ext(&out).as_deref() {
        Some("docx") => {
            let paragraphs: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            docio::save_docx(&out, &paragraphs)
        }
        _ => docio::save_text(&out, &text),
    };
    res.map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = models::resolve(&app.handle());
            let engine = Engine::new(PiiPaths {
                model: paths.ner_model.clone(),
                tokenizer: paths.ner_tokenizer.clone(),
                labels: paths.ner_labels.clone(),
                llm_model: paths.llm_model.clone(),
                llm_tokenizer: paths.llm_tokenizer.clone(),
            });
            app.manage(Backend {
                engine,
                transcriber: Mutex::new(Transcriber::new()),
                transcript: Mutex::new(None),
                summarizer: Mutex::new(None),
                paths,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_whisper_models,
            download_whisper_model,
            list_summary_models,
            list_summary_templates,
            download_summary_model,
            summarize,
            transcribe,
            save_recording,
            update_transcript,
            save_project,
            open_project,
            anonymize,
            anonymized_segments,
            export_transcript,
            save_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
