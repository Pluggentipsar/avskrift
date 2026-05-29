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
mod transcribe;
mod transcript;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use engine::{AnalyzeResult, Engine, ModelPaths as PiiPaths};
use models::{ModelPaths, WhisperModelInfo};
use pii::Category;
use tauri::{AppHandle, Emitter, Manager, State};
use transcribe::Transcriber;
use transcript::Transcript;

/// All long-lived backend state, managed by Tauri.
struct Backend {
    engine: Engine,
    transcriber: Mutex<Transcriber>,
    /// The most recent transcript (timings + speakers), held for export.
    transcript: Mutex<Option<Transcript>>,
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
    let raw = {
        let mut tr = backend.transcriber.lock().unwrap();
        tr.transcribe(&args.model, &model_path, &audio.samples, &args.language, &progress)?
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
        Some("vtt") => docio::save_text(&out, &transcript.to_vtt(texts, labels))?,
        Some("docx") => docio::save_docx(&out, &transcript.to_docx_paragraphs(texts, labels))?,
        _ => docio::save_text(&out, &transcript.to_text(texts, labels))?, // txt / default
    }
    Ok(())
}

fn ext(p: &Path) -> Option<String> {
    p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase())
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
                paths,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_whisper_models,
            download_whisper_model,
            transcribe,
            anonymize,
            anonymized_segments,
            export_transcript,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
