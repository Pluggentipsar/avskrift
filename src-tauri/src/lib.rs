//! Tauri command surface for Avskrift: download/list models, transcribe (+diarise), anonymise the
//! transcript, and export. Heavy work runs on a blocking thread pool via `tauri::async_runtime`.

mod aec;
mod ai;
mod align;
mod audio;
mod capture;
mod diarize;
mod docio;
mod download;
mod engine;
mod jobs;
mod llm;
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
    /// The live meeting recording (dual WASAPI streams), while one is in progress.
    meeting: Mutex<Option<capture::MeetingCapture>>,
    /// Background worker that transcribes meeting chunks live; joined on stop. It RETURNS the
    /// utterances it accumulated (rather than writing shared state), so finalisation can run detached
    /// while a brand-new meeting starts without the two racing over a shared buffer.
    meeting_worker: Mutex<Option<std::thread::JoinHandle<Vec<transcript::Utterance>>>>,
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
        let url = models::whisper_url(&id).ok_or_else(|| format!("okänd modell: {id}"))?.to_string();
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
    summarize::TEMPLATES.iter().map(|t| TemplateInfo { id: t.id.to_string(), label: t.label.to_string() }).collect()
}

/// Download a summary model (GGUF + tokenizer) by id, emitting `avskrift:download` progress.
#[tauri::command]
async fn download_summary_model(app: AppHandle, id: String) -> Result<(), String> {
    let (gguf_url, tok_url, gguf_dest, tok_dest) = {
        let backend = app.state::<Backend>();
        let (gguf_url, tok_url) = models::summary_urls(&id).ok_or_else(|| format!("okänd modell: {id}"))?;
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
        return Err(anyhow::anyhow!("Sammanfattningsmodellen '{}' är inte nedladdad. Hämta den först.", args.model));
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

    let transcript =
        Transcript { utterances, language: args.language.clone(), model: args.model.clone(), diarized: args.diarize };
    *backend.transcript.lock().unwrap() = Some(transcript.clone());
    progress("Klar");
    Ok(transcript)
}

// ---- Meeting capture (dual-stream: mic = "Jag", system loopback = "Mötet") ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartMeetingArgs {
    /// Whisper model id to transcribe live chunks with.
    model: String,
    /// ISO code or "auto".
    language: String,
    /// Transcribe live during the meeting (true) or only after stop (false — gentler on weak CPUs).
    #[serde(default)]
    live: bool,
}

/// Begin recording a digital meeting: the local microphone and the system/render loopback are
/// captured as two separate source streams (see [`capture`]). A background worker transcribes
/// chunks live and emits `avskrift:meeting-utterance` events. Returns once both streams are open;
/// errors if a recording is already running, the model is missing, or an endpoint can't be opened.
///
/// Async + `spawn_blocking`: opening the two WASAPI endpoints blocks until both report ready, and a
/// synchronous command would run that on the WebView2 main thread and freeze the UI while a slow or
/// contended audio device initialises. Off-thread keeps the window responsive during start.
#[tauri::command]
async fn start_meeting(app: AppHandle, args: StartMeetingArgs) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || run_start_meeting(&app, args)).await.map_err(|e| e.to_string())?
}

fn run_start_meeting(app: &AppHandle, args: StartMeetingArgs) -> Result<(), String> {
    let backend = app.state::<Backend>();
    let mut slot = backend.meeting.lock().unwrap();
    if slot.is_some() {
        return Err("En mötesinspelning pågår redan.".to_string());
    }
    let model_path = backend.paths.whisper_file(&args.model);
    if !model_path.exists() {
        return Err(format!("Whisper-modellen '{}' är inte nedladdad. Hämta den först.", args.model));
    }

    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mic_wav = backend.paths.meetings_dir.join(format!("mote-{ts}-mic.wav"));
    let sys_wav = backend.paths.meetings_dir.join(format!("mote-{ts}-system.wav"));

    let (cap, chunks) = capture::MeetingCapture::start(mic_wav, sys_wav)?;

    if args.live {
        // One worker drains chunks from BOTH streams and transcribes them serially on the shared
        // Transcriber, emitting each utterance live and accumulating it for the final transcript.
        let worker_app = app.clone();
        let started = std::time::Instant::now();
        let worker = std::thread::spawn(move || meeting_worker(worker_app, chunks, args.model, args.language, started));
        *backend.meeting_worker.lock().unwrap() = Some(worker);
    } else {
        // "Efter mötet"-läge: capture only; both WAVs are transcribed on stop. Dropping the chunk
        // receiver makes the capture threads' sends cheap no-ops (no live worker, no GPU load).
        drop(chunks);
        *backend.meeting_worker.lock().unwrap() = None;
    }

    *slot = Some(cap);
    Ok(())
}

/// Transcribe live meeting chunks until the capture channel closes (recording stopped). Each
/// utterance is emitted to the UI live and accumulated into the returned vec, which `stop_meeting`
/// collects via `JoinHandle::join` to finalise — no shared state, so a new meeting can start
/// immediately while this one's finalisation runs in the background.
fn meeting_worker(
    app: AppHandle,
    chunks: capture::ChunkReceiver,
    model: String,
    language: String,
    started: std::time::Instant,
) -> Vec<transcript::Utterance> {
    let backend = app.state::<Backend>();
    let model_path = backend.paths.whisper_file(&model);
    let noop = |_: &str| {};
    let mut warned_lag = false;
    let mut utterances: Vec<transcript::Utterance> = Vec::new();

    while let Ok(chunk) = chunks.recv() {
        // Normal latency is ~one chunk length; >30 s behind means the machine can't keep up live.
        // Warn the UI once so it can suggest a smaller model or the "efter mötet" mode.
        if !warned_lag && started.elapsed().as_secs_f64() - chunk.start_s > 30.0 {
            warned_lag = true;
            let _ = app.emit("avskrift:meeting-lag", true);
        }
        let samples = audio::resample_to_16k(&chunk.samples, chunk.src_rate);
        if samples.is_empty() {
            continue;
        }
        let raw = {
            let mut tr = backend.transcriber.lock().unwrap();
            match tr.transcribe(&model, &model_path, &samples, &language, false, false, &noop, |_p| {}) {
                Ok(r) => r,
                Err(_) => continue,
            }
        };
        let label = match chunk.source {
            capture::Source::Mic => "Jag",
            capture::Source::Meeting => "Mötet",
        };
        for seg in raw {
            let start = seg.start + chunk.start_s;
            let end = seg.end + chunk.start_s;
            let _ = app.emit(
                "avskrift:meeting-utterance",
                serde_json::json!({ "source": label, "start": start, "end": end, "text": seg.text }),
            );
            utterances.push(transcript::Utterance {
                start,
                end,
                speaker: Some(label.to_string()),
                text: seg.text,
                words: Vec::new(),
            });
        }
    }
    utterances
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct StopMeetingArgs {
    /// Whisper model id to transcribe both streams with (used only in "efter mötet" mode).
    model: String,
    /// ISO code or "auto".
    language: String,
    /// Frontend-generated id correlating this meeting to its saved project; echoed back in the
    /// `avskrift:meeting-done` / `avskrift:meeting-failed` event so the UI knows which one finished.
    token: String,
}

/// Stop the meeting and finalise it in the BACKGROUND. The capture is stopped synchronously (so the
/// WASAPI endpoints are freed and the UI can immediately start a new meeting), then transcription +
/// echo-cancel + mix run on a detached task that emits `avskrift:meeting-done` with the finished
/// transcript and file paths (or `avskrift:meeting-failed` on error), tagged with `args.token`. The
/// finalised meeting is NOT written into the shared transcript slot — the frontend saves it straight
/// to history, so it never clobbers whatever project the user moved on to while it transcribed.
#[tauri::command]
async fn stop_meeting(app: AppHandle, args: StopMeetingArgs) -> Result<(), String> {
    // Synchronous + quick: stop both capture streams and take the live worker, under the meeting
    // locks, so a concurrent start_meeting can't interleave. Errors (e.g. no meeting running) surface
    // to the caller immediately.
    let app_stop = app.clone();
    let (worker, files) = tauri::async_runtime::spawn_blocking(move || stop_capture(&app_stop))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    // Detached: the heavy transcription/mix runs to completion on its own blocking task (dropping the
    // handle does not cancel it) and reports via events. The meeting/worker slots are already free, so
    // the user can start the next meeting right away.
    let _ = tauri::async_runtime::spawn_blocking(move || finalize_meeting(&app, worker, files, args));
    Ok(())
}

/// Stop both capture streams (freeing the audio endpoints) and take the live worker handle, all under
/// the meeting locks so a concurrent `start_meeting` can't interleave. Returns the worker (if live)
/// plus the finished source files.
fn stop_capture(
    app: &AppHandle,
) -> anyhow::Result<(Option<std::thread::JoinHandle<Vec<transcript::Utterance>>>, capture::MeetingFiles)> {
    let backend = app.state::<Backend>();
    let cap = backend.meeting.lock().unwrap().take().ok_or_else(|| anyhow::anyhow!("Ingen mötesinspelning pågår."))?;
    let worker = backend.meeting_worker.lock().unwrap().take();
    emit(app, "Avslutar inspelning…");
    let files = cap.stop().map_err(|e| anyhow::anyhow!(e))?;
    Ok((worker, files))
}

/// Background finalisation: build the speaker-attributed transcript and the playback mix, then emit
/// the result tagged with the meeting's token. Runs detached from any shared backend state.
fn finalize_meeting(
    app: &AppHandle,
    worker: Option<std::thread::JoinHandle<Vec<transcript::Utterance>>>,
    files: capture::MeetingFiles,
    args: StopMeetingArgs,
) {
    match build_meeting(app, worker, &files, &args) {
        Ok((transcript, mix_wav_path)) => {
            let _ = app.emit(
                "avskrift:meeting-done",
                serde_json::json!({
                    "token": args.token,
                    "transcript": transcript,
                    "micWavPath": files.mic_wav,
                    "systemWavPath": files.sys_wav,
                    "mixWavPath": mix_wav_path,
                    "durationS": files.duration_s,
                }),
            );
        }
        Err(e) => {
            let _ = app.emit("avskrift:meeting-failed", serde_json::json!({ "token": args.token, "error": e.to_string() }));
        }
    }
}

/// Build a meeting's transcript ("Jag" / "Mötet") from the live worker's utterances, or by batch-
/// transcribing both source WAVs in "efter mötet" mode, plus the echo-cleaned playback mix.
fn build_meeting(
    app: &AppHandle,
    worker: Option<std::thread::JoinHandle<Vec<transcript::Utterance>>>,
    files: &capture::MeetingFiles,
    args: &StopMeetingArgs,
) -> anyhow::Result<(Transcript, Option<String>)> {
    let utterances = if let Some(handle) = worker {
        // LIVE mode: the capture threads ended → chunk senders dropped → the worker drains the last
        // queued chunks and exits, returning everything it transcribed.
        emit(app, "Slutför transkribering…");
        let live = handle.join().map_err(|_| anyhow::anyhow!("transkriberingstråden kraschade"))?;
        align::drop_meeting_echo(align::from_labeled(live))
    } else {
        // "Efter mötet"-läge: nothing was transcribed live — transcribe both source WAVs now.
        transcribe_meeting_wavs(app, &files.mic_wav, &files.sys_wav, &args.model, &args.language, false, true)?
    };

    let transcript =
        Transcript { utterances, language: args.language.clone(), model: args.model.clone(), diarized: true };

    // Build the mixed playback track (your echo-cleaned mic + the meeting), so you hear yourself.
    emit(app, "Skapar uppspelningsmix…");
    let mix_wav_path = write_meeting_mix(&files.mic_wav, &files.sys_wav, true);
    Ok((transcript, mix_wav_path))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DiarizeMeetingArgs {
    system_wav_path: String,
    num_speakers: Option<usize>,
}

/// Split the current meeting transcript's "Mötet" utterances into distinct speakers (TALARE_1..) by
/// diarising the system-audio WAV; "Jag" (the mic) is left untouched.
#[tauri::command]
async fn diarize_meeting(app: AppHandle, args: DiarizeMeetingArgs) -> Result<Transcript, String> {
    tauri::async_runtime::spawn_blocking(move || run_diarize_meeting(&app, args))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn run_diarize_meeting(app: &AppHandle, args: DiarizeMeetingArgs) -> anyhow::Result<Transcript> {
    let backend = app.state::<Backend>();
    let progress = |m: &str| emit(app, m);

    let current = backend
        .transcript
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Inget mötestranskript att separera."))?;

    progress("Läser mötesljud…");
    let audio = audio::load(Path::new(&args.system_wav_path))?;
    progress("Identifierar mötesröster…");
    let turns = diarize::diarize(
        &backend.paths.diar_segmentation,
        &backend.paths.diar_embedding,
        &audio.samples,
        args.num_speakers,
        &progress,
    )?;

    let transcript = Transcript {
        utterances: align::split_meeting_speakers(current.utterances, &turns, "Mötet"),
        language: current.language,
        model: current.model,
        diarized: true,
    };
    *backend.transcript.lock().unwrap() = Some(transcript.clone());
    progress("Klar");
    Ok(transcript)
}

/// Batch-transcribe a meeting's two source WAVs (mic = "Jag", system = "Mötet") with `model` and
/// merge them into one time-ordered, speaker-labelled list. A missing or empty WAV simply
/// contributes nothing. Shared by `stop_meeting` ("efter mötet") and `retranscribe_meeting`.
/// Derive the mixed-playback path `…-mix.wav` from the system WAV path `…-system.wav`.
fn mix_path_for(sys_wav: &str) -> Option<String> {
    if sys_wav.ends_with("-system.wav") {
        return Some(sys_wav.replace("-system.wav", "-mix.wav"));
    }
    let p = Path::new(sys_wav);
    Some(p.with_file_name(format!("{}-mix.wav", p.file_stem()?.to_str()?)).to_string_lossy().to_string())
}

/// Write a single mixed playback WAV (your echo-cleaned mic + the meeting) so a recording can be
/// played back as one track with both voices and no echo. Returns the written path, or None if the
/// source streams are missing/empty or writing failed (playback then falls back to the system WAV).
fn write_meeting_mix(mic_wav: &str, sys_wav: &str, echo_cancel: bool) -> Option<String> {
    let mic = audio::load(Path::new(mic_wav)).ok().filter(|a| !a.samples.is_empty())?.samples;
    let sys = audio::load(Path::new(sys_wav)).ok().filter(|a| !a.samples.is_empty())?.samples;
    let cleaned = if echo_cancel { aec::cancel_echo(&mic, &sys) } else { mic };
    let mixed = aec::mix(&cleaned, &sys);
    let out = mix_path_for(sys_wav)?;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: audio::TARGET_SR,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(&out, spec).ok()?;
    for &s in &mixed {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).ok()?;
    }
    w.finalize().ok()?;
    Some(out)
}

fn transcribe_meeting_wavs(
    app: &AppHandle,
    mic_wav: &str,
    sys_wav: &str,
    model: &str,
    language: &str,
    word_timestamps: bool,
    echo_cancel: bool,
) -> anyhow::Result<Vec<transcript::Utterance>> {
    let backend = app.state::<Backend>();
    let model_path = backend.paths.whisper_file(model);
    let progress = |m: &str| emit(app, m);

    // Load both source streams (mono 16 kHz); a missing/empty file → None.
    let load = |wav: &str| -> Option<Vec<f32>> {
        let p = Path::new(wav);
        if !p.exists() {
            return None;
        }
        audio::load(p).ok().filter(|a| !a.samples.is_empty()).map(|a| a.samples)
    };
    let mut mic_samples = load(mic_wav);
    let sys_samples = load(sys_wav);

    // Echo cancellation: strip the meeting audio that leaked into the mic (speakers → mic) before
    // transcribing "Jag", so the other person stops appearing in your track. Falls back internally
    // to the untouched mic if it didn't help.
    if echo_cancel {
        if let (Some(mic), Some(sys)) = (mic_samples.as_ref(), sys_samples.as_ref()) {
            progress("Tar bort eko ur mikrofonen…");
            mic_samples = Some(aec::cancel_echo(mic, sys));
        }
    }

    let transcribe_samples =
        |samples: &[f32], label: &str, base: i32, span: i32| -> anyhow::Result<Vec<transcript::Utterance>> {
            let app_pct = app.clone();
            let raw = {
                let mut tr = backend.transcriber.lock().unwrap();
                tr.transcribe(model, &model_path, samples, language, word_timestamps, false, &progress, move |p| {
                    let _ = app_pct.emit("avskrift:percent", base + span * p / 100);
                })?
            };
            Ok(align::without_speakers(raw)
                .into_iter()
                .map(|mut u| {
                    u.speaker = Some(label.to_string());
                    u
                })
                .collect())
        };

    let mut utts = Vec::new();
    if let Some(mic) = mic_samples.as_ref() {
        progress("Transkriberar din röst…");
        utts.extend(transcribe_samples(mic, "Jag", 0, 50)?);
    }
    if let Some(sys) = sys_samples.as_ref() {
        progress("Transkriberar mötet…");
        utts.extend(transcribe_samples(sys, "Mötet", 50, 50)?);
    }
    Ok(align::drop_meeting_echo(align::from_labeled(utts)))
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetranscribeMeetingArgs {
    mic_wav_path: Option<String>,
    system_wav_path: Option<String>,
    /// Whisper model id to re-transcribe with — typically a larger/better one than the live run.
    model: String,
    /// ISO code or "auto".
    language: String,
    /// Remove the meeting audio that bled into the mic (speakers → mic) before transcribing "Jag".
    #[serde(default)]
    echo_cancel: bool,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RetranscribeResult {
    transcript: Transcript,
    /// Rebuilt mixed playback track (mic + meeting, echo removed), if it could be built.
    mix_wav_path: Option<String>,
}

/// Re-transcribe a finished meeting from its saved source WAVs with a chosen Whisper model: a full
/// batch pass (better than the live/chunked run, especially with a larger model), re-merged into
/// "Jag" / "Mötet" and stored. Run `diarize_meeting` afterwards to re-split "Mötet" into speakers.
#[tauri::command]
async fn retranscribe_meeting(app: AppHandle, args: RetranscribeMeetingArgs) -> Result<RetranscribeResult, String> {
    tauri::async_runtime::spawn_blocking(move || run_retranscribe_meeting(&app, args))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn run_retranscribe_meeting(app: &AppHandle, args: RetranscribeMeetingArgs) -> anyhow::Result<RetranscribeResult> {
    let mic = args.mic_wav_path.unwrap_or_default();
    let sys = args.system_wav_path.unwrap_or_default();
    if mic.is_empty() && sys.is_empty() {
        anyhow::bail!("Det här mötet har inga sparade ljudfiler att transkribera om.");
    }
    let utterances = transcribe_meeting_wavs(app, &mic, &sys, &args.model, &args.language, true, args.echo_cancel)?;
    if utterances.is_empty() {
        anyhow::bail!("Inget ljud kunde transkriberas – ljudfilerna saknas eller är tomma.");
    }
    let transcript =
        Transcript { utterances, language: args.language.clone(), model: args.model.clone(), diarized: true };
    let backend = app.state::<Backend>();
    *backend.transcript.lock().unwrap() = Some(transcript.clone());
    // Rebuild the mixed playback track to match the re-transcription's echo-cancel choice.
    let mix_wav_path =
        if mic.is_empty() || sys.is_empty() { None } else { write_meeting_mix(&mic, &sys, args.echo_cancel) };
    emit(app, "Klar");
    Ok(RetranscribeResult { transcript, mix_wav_path })
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AskArgs {
    question: String,
    transcript_text: String,
    model: String,
}

/// Answer a free-text question strictly from a transcript (reuses the summariser's Qwen model).
#[tauri::command]
async fn ask_transcript(app: AppHandle, args: AskArgs) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || run_ask(&app, args))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn run_ask(app: &AppHandle, args: AskArgs) -> anyhow::Result<String> {
    let backend = app.state::<Backend>();
    let progress = |m: &str| emit(app, m);

    let (gguf, tok) = backend.paths.summary_files(&args.model);
    if !gguf.exists() || !tok.exists() {
        return Err(anyhow::anyhow!("Modellen '{}' är inte nedladdad. Hämta den först.", args.model));
    }
    let mut guard = backend.summarizer.lock().unwrap();
    let needs_load = !matches!(&*guard, Some((cur, _)) if *cur == args.model);
    if needs_load {
        progress(&format!("Laddar modell ({})…", args.model));
        *guard = Some((args.model.clone(), Summarizer::load(&gguf, &tok)?));
    }
    let (_, summarizer) = guard.as_ref().unwrap();
    summarizer.answer(&args.question, &args.transcript_text, &progress)
}

// ---- Recording ----

/// Persist a recording captured in the webview to a temp file and return its path, so the existing
/// `transcribe` pipeline can pick it up like any other audio file. The webview sends the 16-bit PCM
/// WAV as a **base64 string** (not a byte-per-element JS array): the old `Vec<u8>` form was serialized
/// to JSON one number per byte, which froze the app on stop for anything longer than a minute or two.
/// Runs the decode + disk write on a blocking thread so a large write can't stall the UI thread.
#[tauri::command]
async fn save_recording(data: String) -> Result<String, String> {
    if data.is_empty() {
        return Err("ingen inspelning att spara".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.as_bytes())
            .map_err(|e| format!("ogiltig inspelningsdata: {e}"))?;
        // A bare WAV header is 44 bytes; anything at or below that carries no audio.
        if bytes.len() <= 44 {
            return Err("inspelningen innehåller inget ljud".to_string());
        }
        let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let path = std::env::temp_dir().join(format!("avskrift-inspelning-{ts}.wav"));
        std::fs::write(&path, &bytes).map_err(|e| format!("kunde inte spara inspelningen: {e}"))?;
        Ok(path.to_string_lossy().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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
    let transcript = guard.as_ref().ok_or_else(|| "det finns inget transkript att spara".to_string())?.clone();
    let project = Project { version: 1, transcript, speaker_labels: args.speaker_labels, audio_path: args.audio_path };
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
        backend.engine.analyze_segments(args.texts, args.enabled, args.terms, args.use_ai, &progress)
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

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManualSpanArgs {
    start: usize,
    end: usize,
    /// Category key (snake_case, e.g. "personnummer", "ovrigt").
    category: String,
    custom: Option<String>,
}

/// Add a manual mask the user created by clicking a word. Returns the rebuilt analysis (span ids
/// are renumbered, so the frontend resets its rejected set).
#[tauri::command]
fn add_manual_span(backend: State<Backend>, args: ManualSpanArgs) -> Result<AnalyzeResult, String> {
    let category: Category =
        serde_json::from_value(serde_json::Value::String(args.category)).map_err(|_| "okänd kategori".to_string())?;
    backend.engine.add_manual_span(args.start, args.end, category, args.custom).map_err(|e| e.to_string())
}

// ---- Standalone de-identify (arbitrary pasted text or a loaded .txt/.md/.docx, no transcript) ----

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeDocArgs {
    /// Pasted text, used when `path` is None.
    #[serde(default)]
    text: Option<String>,
    /// A .txt/.md/.docx file to load and analyse instead of `text`.
    #[serde(default)]
    path: Option<String>,
    enabled: Vec<Category>,
    terms: Vec<String>,
    use_ai: bool,
}

/// Analyse standalone text or a document (not the transcript). Sets `engine.last`, so
/// `copy_anonymized`/`export_anonymized` then work exactly like the transcript path does.
#[tauri::command]
async fn analyze_document(app: AppHandle, args: AnalyzeDocArgs) -> Result<AnalyzeResult, String> {
    tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<AnalyzeResult> {
        let backend = app.state::<Backend>();
        let progress = |m: &str| emit(&app, m);
        let AnalyzeDocArgs { text, path, enabled, terms, use_ai } = args;
        match (path, text) {
            (Some(p), _) => backend.engine.analyze_file(PathBuf::from(p), enabled, terms, use_ai, &progress),
            (None, Some(t)) => backend.engine.analyze_text(t, enabled, terms, use_ai, &progress),
            (None, None) => Err(anyhow::anyhow!("ingen text eller fil angiven")),
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

/// Load a .txt/.md/.docx into plain text (the summarise file source + a source preview).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadedDocInfo {
    text: String,
    has_tables: bool,
}

#[tauri::command]
fn load_document(path: String) -> Result<LoadedDocInfo, String> {
    let doc = docio::load(Path::new(&path)).map_err(|e| e.to_string())?;
    Ok(LoadedDocInfo { text: doc.text, has_tables: doc.has_tables })
}

/// Masked full text of the last standalone analysis (for copy-to-clipboard).
#[tauri::command]
fn copy_anonymized(backend: State<Backend>, rejected: Vec<usize>) -> Result<String, String> {
    backend.engine.anonymized_text(rejected).map_err(|e| e.to_string())
}

/// Export the last standalone analysis to .txt or .docx (format chosen by extension).
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportAnonArgs {
    path: String,
    rejected: Vec<usize>,
}

#[tauri::command]
fn export_anonymized(backend: State<Backend>, args: ExportAnonArgs) -> Result<(), String> {
    backend.engine.export(PathBuf::from(args.path), args.rejected).map_err(|e| e.to_string())
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
    let transcript = guard.as_ref().ok_or_else(|| anyhow::anyhow!("det finns inget transkript att exportera"))?;

    // When anonymising, fetch masked text per utterance so timestamps & speakers are preserved.
    let masked: Option<Vec<String>> =
        if args.anonymize { Some(backend.engine.anonymized_segments(args.rejected)?) } else { None };
    let texts = masked.as_deref();
    let labels = &args.speaker_labels;
    let out = PathBuf::from(&args.path);

    match ext(&out).as_deref() {
        Some("srt") => docio::save_text(&out, &transcript.to_srt(texts, labels))?,
        Some("vtt") if args.word_level && !args.anonymize => docio::save_text(&out, &transcript.to_vtt_words(labels))?,
        Some("vtt") => docio::save_text(&out, &transcript.to_vtt(texts, labels))?,
        Some("docx") if args.timestamps => docio::save_docx(&out, &transcript.to_docx_paragraphs_timed(texts, labels))?,
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

// ---- Jobs / history (auto-saved past work) ----

#[tauri::command]
fn list_jobs(backend: State<Backend>) -> Vec<jobs::JobMeta> {
    jobs::list(&backend.paths.jobs_dir)
}

/// Full-text search over jobs (title, category, transcript, summary, source). Empty query = all.
#[tauri::command]
fn search_jobs(backend: State<Backend>, query: String) -> Vec<jobs::JobMeta> {
    jobs::search(&backend.paths.jobs_dir, &query)
}

#[tauri::command]
fn save_job(backend: State<Backend>, job: jobs::Job) -> Result<(), String> {
    let mut job = job;
    // Ordinary saves (transcript edits etc.) don't carry the category; keep the one set in History.
    if job.category.is_empty() {
        if let Ok(existing) = jobs::open(&backend.paths.jobs_dir, &job.id) {
            job.category = existing.category;
        }
    }
    jobs::save(&backend.paths.jobs_dir, &job).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_job(backend: State<Backend>, id: String) -> Result<jobs::Job, String> {
    jobs::open(&backend.paths.jobs_dir, &id).map_err(|e| e.to_string())
}

/// Rename a folder or collapse it into its parent (delete-folder) by rewriting the category prefix
/// on every job under it. `to` = "" moves jobs to the root.
#[tauri::command]
fn move_folder(backend: State<Backend>, from: String, to: String) -> Result<(), String> {
    jobs::move_folder(&backend.paths.jobs_dir, &from, &to).map_err(|e| e.to_string())?;
    jobs::move_task_folder(&backend.paths.tasks_file, &from, &to).map_err(|e| e.to_string())
}

/// Rename / recategorise a job. Does not bump `updated_at`, so the History order is preserved.
#[tauri::command]
fn update_job_meta(backend: State<Backend>, id: String, title: String, category: String) -> Result<(), String> {
    let mut job = jobs::open(&backend.paths.jobs_dir, &id).map_err(|e| e.to_string())?;
    job.title = title;
    job.category = category;
    jobs::save(&backend.paths.jobs_dir, &job).map_err(|e| e.to_string())
}

/// Delete a job's audio to reclaim space, keeping the transcript and everything else. Only files
/// Avskrift created are removed from disk; the user's own uploads are just dereferenced.
#[tauri::command]
fn delete_job_audio(backend: State<Backend>, id: String) -> Result<jobs::Job, String> {
    let mut job = jobs::open(&backend.paths.jobs_dir, &id).map_err(|e| e.to_string())?;
    remove_avskrift_audio(&job, &backend.paths.meetings_dir);
    job.audio_path = None;
    job.mic_wav_path = None;
    job.mix_wav_path = None;
    jobs::save(&backend.paths.jobs_dir, &job).map_err(|e| e.to_string())?;
    Ok(job)
}

#[tauri::command]
fn delete_job(backend: State<Backend>, id: String) -> Result<(), String> {
    if let Ok(job) = jobs::open(&backend.paths.jobs_dir, &id) {
        remove_avskrift_audio(&job, &backend.paths.meetings_dir);
    }
    jobs::delete(&backend.paths.jobs_dir, &id).map_err(|e| e.to_string())
}

// ============================================================================
// Cross-project åtaganden (action items) overview
// ============================================================================

/// Every action across all jobs, plus the free-standing task store, flattened for the overview.
#[tauri::command]
fn list_all_actions(backend: State<Backend>) -> Vec<jobs::ActionRow> {
    jobs::all_actions(&backend.paths.jobs_dir, &backend.paths.tasks_file)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateJobActionArgs {
    job_id: String,
    index: usize,
    action: jobs::Action,
    updated_at: String,
}

/// Toggle/edit one job's action straight from the overview (writes back to that job's file).
#[tauri::command]
fn update_job_action(backend: State<Backend>, args: UpdateJobActionArgs) -> Result<(), String> {
    jobs::set_job_action(&backend.paths.jobs_dir, &args.job_id, args.index, args.action, &args.updated_at)
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddJobActionArgs {
    job_id: String,
    action: jobs::Action,
    updated_at: String,
}

/// Add a new action to an existing job from the overview.
#[tauri::command]
fn add_job_action(backend: State<Backend>, args: AddJobActionArgs) -> Result<(), String> {
    jobs::add_job_action(&backend.paths.jobs_dir, &args.job_id, args.action, &args.updated_at)
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteJobActionArgs {
    job_id: String,
    index: usize,
    updated_at: String,
}

/// Remove one job's action from the overview.
#[tauri::command]
fn delete_job_action(backend: State<Backend>, args: DeleteJobActionArgs) -> Result<(), String> {
    jobs::delete_job_action(&backend.paths.jobs_dir, &args.job_id, args.index, &args.updated_at)
        .map_err(|e| e.to_string())
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskArgs {
    task: jobs::StandaloneTask,
}

/// Create a free-standing task (not tied to any job).
#[tauri::command]
fn add_standalone_task(backend: State<Backend>, args: TaskArgs) -> Result<(), String> {
    jobs::add_task(&backend.paths.tasks_file, args.task).map_err(|e| e.to_string())
}

/// Toggle/edit a free-standing task.
#[tauri::command]
fn update_standalone_task(backend: State<Backend>, args: TaskArgs) -> Result<(), String> {
    jobs::update_task(&backend.paths.tasks_file, args.task).map_err(|e| e.to_string())
}

/// Delete a free-standing task by id.
#[tauri::command]
fn delete_standalone_task(backend: State<Backend>, id: String) -> Result<(), String> {
    jobs::delete_task(&backend.paths.tasks_file, &id).map_err(|e| e.to_string())
}

/// Remove audio files that Avskrift itself created — meeting WAVs in `meetings_dir`, or temporary
/// in-app recordings named `avskrift-inspelning-*`. The user's own uploaded files are left untouched.
fn remove_avskrift_audio(job: &jobs::Job, meetings_dir: &Path) {
    let temp = std::env::temp_dir();
    for p in [job.audio_path.as_deref(), job.mic_wav_path.as_deref(), job.mix_wav_path.as_deref()].into_iter().flatten()
    {
        let path = Path::new(p);
        let ours = path.starts_with(meetings_dir)
            || (path.starts_with(&temp)
                && path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("avskrift-inspelning-")));
        if ours && path.exists() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let paths = models::resolve(app.handle());
            // PII "Övrigt (AI)" reuses the 1.5B Qwen — the bundled copy if present, otherwise the
            // downloadable summary 1.5B, so the lean installer can enable it via one download.
            let (pii_llm, pii_llm_tok) = paths.summary_files("qwen2.5-1.5b");
            let engine = Engine::new(PiiPaths {
                model: paths.ner_model.clone(),
                tokenizer: paths.ner_tokenizer.clone(),
                labels: paths.ner_labels.clone(),
                llm_model: pii_llm,
                llm_tokenizer: pii_llm_tok,
            });
            app.manage(Backend {
                engine,
                transcriber: Mutex::new(Transcriber::new()),
                transcript: Mutex::new(None),
                summarizer: Mutex::new(None),
                meeting: Mutex::new(None),
                meeting_worker: Mutex::new(None),
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
            start_meeting,
            stop_meeting,
            diarize_meeting,
            retranscribe_meeting,
            ask_transcript,
            update_transcript,
            save_project,
            open_project,
            anonymize,
            anonymized_segments,
            add_manual_span,
            analyze_document,
            load_document,
            copy_anonymized,
            export_anonymized,
            export_transcript,
            save_summary,
            list_jobs,
            search_jobs,
            save_job,
            open_job,
            update_job_meta,
            move_folder,
            delete_job_audio,
            delete_job,
            list_all_actions,
            update_job_action,
            add_job_action,
            delete_job_action,
            add_standalone_task,
            update_standalone_task,
            delete_standalone_task,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
