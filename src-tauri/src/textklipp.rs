//! Textklipp projects: a video (or audio) file, its transcript with exact word times, a playback
//! proxy and the edit list. Each project lives in its own folder under `textklipp/<id>/`
//! (project.json, audio16k.wav, proxy.mp4) so deleting a project frees all its large files.
//! The original file is never modified. See docs/TEXTKLIPP-PLAN.md.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, ensure, Context, Result};
use avskrift_textklipp::{ffmpeg, EditList, MediaInfo};
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};

use crate::{models::ModelPaths, storage, transcript::Transcript, work};

pub const PROJECT_FILE: &str = "project.json";
pub const AUDIO_FILE: &str = "audio16k.wav";
pub const PROXY_FILE: &str = "proxy.mp4";
/// Sound-block ids start here so they never collide with word ids (word index in the transcript).
pub const SOUND_ID_BASE: u32 = 1_000_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Importing,
    /// Transcript is ready; the proxy may still be missing (`proxy == None`).
    Ready,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sound {
    pub id: u32,
    pub start: f64,
    pub end: f64,
    pub heard: String,
    /// Model confidence of `heard`; the editor shows the reading only when it is high.
    #[serde(default)]
    pub score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub version: u32,
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub source_path: String,
    pub media: MediaInfo,
    pub status: Status,
    #[serde(default)]
    pub error: Option<String>,
    /// Transcript with stable word ids = index over all utterances' words in order.
    #[serde(default)]
    pub transcript: Option<Transcript>,
    /// "exakta" (acoustic alignment) or "whisper" (fallback; not reliable for cutting).
    #[serde(default)]
    pub word_times: String,
    #[serde(default)]
    pub sounds: Vec<Sound>,
    #[serde(default)]
    pub pauses: Vec<(f64, f64)>,
    /// File name of the proxy inside the project folder, once created.
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub edits: EditList,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    pub id: String,
    pub title: String,
    pub updated_at: String,
    pub duration: f64,
    pub status: Status,
    pub has_proxy: bool,
}

// ---------------------------------------------------------------- FFmpeg

pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

fn command(program: &Path) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flashing up
    }
    c
}

pub fn probe(tools: &Tools, path: &Path) -> Result<MediaInfo> {
    let out = command(&tools.ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .output()
        .context("FFprobe kunde inte startas")?;
    ensure!(out.status.success(), "Filen kunde inte läsas som ljud eller video: {}", String::from_utf8_lossy(&out.stderr).trim());
    let info = MediaInfo::from_ffprobe(&String::from_utf8_lossy(&out.stdout))?;
    ensure!(info.audio.is_some(), "Filen saknar ljudspår och kan inte transkriberas.");
    Ok(info)
}

/// First H.264 encoder that really encodes on this machine (cached for the session).
pub fn encoder(tools: &Tools) -> Result<&'static str> {
    static CHOSEN: OnceCell<&'static str> = OnceCell::new();
    CHOSEN
        .get_or_try_init(|| {
            ffmpeg::H264_ENCODERS
                .iter()
                .copied()
                .find(|e| {
                    command(&tools.ffmpeg)
                        .args(ffmpeg::encoder_probe_args(e))
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .is_ok_and(|s| s.success())
                })
                .ok_or_else(|| anyhow!("Ingen H.264-kodare fungerar på den här datorn."))
        })
        .copied()
}

/// Run FFmpeg with `-progress pipe:1`; reports 0..=100 over `duration` and kills it on cancel.
fn run_ffmpeg(tools: &Tools, args: &[String], duration: f64, pct: &dyn Fn(i32)) -> Result<()> {
    run_ffmpeg_until(tools, args, duration, pct, &|| work::check().is_err())
}

/// As [`run_ffmpeg`], with an explicit stop check: worker threads have no work context of their
/// own, so they pass the request's cancel token (and a "sibling failed" flag) instead.
fn run_ffmpeg_until(tools: &Tools, args: &[String], duration: f64, pct: &dyn Fn(i32), stop: &dyn Fn() -> bool) -> Result<()> {
    let mut child = command(&tools.ffmpeg)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("FFmpeg kunde inte startas")?;
    let stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut s);
        s
    });
    for line in BufReader::new(stdout).lines() {
        if stop() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!(work::Cancelled));
        }
        if let Some(t) = line.ok().as_deref().and_then(ffmpeg::progress_seconds) {
            pct(((t / duration.max(0.001)) * 100.0).clamp(0.0, 100.0) as i32);
        }
    }
    let status = child.wait()?;
    if stop() {
        return Err(anyhow!(work::Cancelled));
    }
    let errors = errors.join().unwrap_or_default();
    if !status.success() {
        eprintln!("AVskrift FFmpeg-fel ({}):\n{errors}", args.last().map_or("", |s| s.as_str()));
    }
    // The last line is often only "Nothing was written…"; the cause (e.g. an encoder that could not
    // start) is the first real error line.
    let cause = errors
        .lines()
        .find(|l| (l.contains("rror") || l.contains("failed") || l.contains("Cannot")) && !l.contains("Nothing was written"))
        .or_else(|| errors.lines().last())
        .unwrap_or("okänt fel");
    ensure!(status.success(), "FFmpeg misslyckades: {}", cause.trim());
    Ok(())
}

pub fn extract_audio(tools: &Tools, src: &Path, dst: &Path, duration: f64, pct: &dyn Fn(i32)) -> Result<()> {
    let args = ffmpeg::audio_args(&src.to_string_lossy(), &dst.to_string_lossy());
    run_ffmpeg(tools, &args, duration, pct)
}

pub fn make_proxy(tools: &Tools, src: &Path, dst: &Path, media: &MediaInfo, pct: &dyn Fn(i32)) -> Result<&'static str> {
    let video = media.video.as_ref().ok_or_else(|| anyhow!("ingen bild att göra proxy av"))?;
    let enc = encoder(tools)?;
    let partial = dst.with_extension("partial.mp4");
    let args = ffmpeg::proxy_args(&src.to_string_lossy(), &partial.to_string_lossy(), enc, video.fps);
    let result = run_ffmpeg(tools, &args, media.duration, pct);
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result?;
    std::fs::rename(&partial, dst)?;
    Ok(enc)
}

// ---------------------------------------------------------------- Import

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportArgs {
    pub path: String,
    pub model: String,
    pub language: String,
    #[serde(default)]
    pub diarize: bool,
    #[serde(default)]
    pub num_speakers: Option<usize>,
}

/// Overall progress 0..=100 across all import stages.
pub type Pct = std::sync::Arc<dyn Fn(i32) + Send + Sync>;

fn stage(pct: &Pct, base: i32, span: i32) -> impl Fn(i32) + Send + Sync + 'static {
    let pct = pct.clone();
    move |p: i32| pct(base + span * p.clamp(0, 100) / 100)
}

/// Import a file: audio (0–5 %), transcription (5–55 %), exact word times (55–65 %), playback
/// proxy (65–100 %). The project is saved after each stage. A cancel before the transcript exists
/// removes the project; during the proxy it keeps the transcript (the proxy can be made later).
pub fn import(paths: &ModelPaths, args: &ImportArgs, progress: &dyn Fn(&str), pct: &Pct) -> Result<Project> {
    let tools = paths.ffmpeg_tools();
    let root = &paths.textklipp_dir;
    let src = PathBuf::from(&args.path);
    check_source(&src)?;
    progress("Läser filen…");
    let media = probe(&tools, &src)?;
    let created = now();
    let mut p = Project {
        version: 1,
        id: new_id(),
        title: title_of(&src),
        created_at: created.clone(),
        updated_at: created,
        source_path: args.path.clone(),
        media: media.clone(),
        status: Status::Importing,
        error: None,
        transcript: None,
        word_times: String::new(),
        sounds: vec![],
        pauses: vec![],
        proxy: None,
        edits: EditList::default(),
    };
    save(root, &p)?;
    let dir = project_dir(root, &p.id)?;

    let text = (|| -> Result<()> {
        progress("Tar ut ljudet…");
        extract_audio(&tools, &src, &dir.join(AUDIO_FILE), media.duration, &stage(pct, 0, 5))?;
        let audio = crate::audio::load(&dir.join(AUDIO_FILE))?;
        let raw = crate::transcribe::Transcriber::new().transcribe(
            &args.model,
            &paths.speech_file(&args.model),
            &audio.samples,
            &args.language,
            true,
            false,
            progress,
            stage(pct, 5, 50),
        )?;
        let mut utterances = if args.diarize {
            let turns = crate::diarize::diarize(
                &paths.diar_segmentation,
                &paths.diar_embedding,
                &audio.samples,
                args.num_speakers,
                progress,
            )?;
            crate::align::with_speakers(raw, &turns)
        } else {
            crate::align::without_speakers(raw)
        };
        p.word_times = "whisper".into();
        if crate::wordalign::ready(&paths.wordalign_dir) && utterances.iter().any(|u| !u.words.is_empty()) {
            match crate::wordalign::refine(&paths.wordalign_dir, &audio.samples, &mut utterances, progress, stage(pct, 55, 10)) {
                Ok(r) => set_alignment(&mut p, &r),
                Err(e) => {
                    work::check()?;
                    progress(&format!("Exakta ordtider kunde inte beräknas ({e}). Whispers ordtider används."));
                }
            }
        }
        p.transcript = Some(Transcript {
            utterances,
            language: args.language.clone(),
            model: args.model.clone(),
            diarized: args.diarize,
        });
        p.status = Status::Ready;
        p.updated_at = now();
        save(root, &p)
    })();
    if let Err(e) = text {
        if e.is::<work::Cancelled>() {
            let _ = delete(root, &p.id);
        } else {
            p.status = Status::Failed;
            p.error = Some(e.to_string());
            let _ = save(root, &p);
        }
        return Err(e);
    }
    if media.video.is_some() {
        build_proxy(paths, &mut p, progress, &stage(pct, 65, 35))?;
    }
    progress("Klar");
    Ok(p)
}

/// Create the playback proxy. A failure is recorded on the project (the transcript stays usable);
/// only a cancel is returned as an error.
pub fn build_proxy(paths: &ModelPaths, p: &mut Project, progress: &dyn Fn(&str), pct: &dyn Fn(i32)) -> Result<()> {
    let dir = project_dir(&paths.textklipp_dir, &p.id)?;
    progress("Skapar uppspelningskopia…");
    match make_proxy(&paths.ffmpeg_tools(), Path::new(&p.source_path), &dir.join(PROXY_FILE), &p.media, pct) {
        Ok(encoder) => {
            eprintln!("AVskrift textklipp: proxy med {encoder}");
            p.proxy = Some(PROXY_FILE.into());
            p.error = None;
        }
        Err(e) if e.is::<work::Cancelled>() => return Err(e),
        Err(e) => p.error = Some(format!("Uppspelningskopian kunde inte skapas: {e}")),
    }
    p.updated_at = now();
    save(&paths.textklipp_dir, p)
}

// ---------------------------------------------------------------- Edit preview

/// Loudness of the most recently previewed project (1 f32 per ms; ~14 MB per hour of audio).
static LOUDNESS: once_cell::sync::Lazy<std::sync::Mutex<Option<(String, avskrift_wordalign::Loudness)>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// Source ranges that remain, in order; playback skips everything else.
    pub keep: Vec<(f64, f64)>,
    pub edited_duration: f64,
}

/// Words (id = index over all words in order) and sound blocks, sorted by start.
pub fn items(p: &Project) -> Vec<avskrift_textklipp::Item> {
    let mut out: Vec<avskrift_textklipp::Item> = p
        .transcript
        .iter()
        .flat_map(|t| t.utterances.iter().flat_map(|u| &u.words))
        .enumerate()
        .map(|(i, w)| avskrift_textklipp::Item { id: i as u32, start: w.start, end: w.end })
        .collect();
    out.extend(p.sounds.iter().map(|s| avskrift_textklipp::Item { id: s.id, start: s.start, end: s.end }));
    out.sort_by(|a, b| a.start.total_cmp(&b.start));
    out
}

/// The same keep ranges an export will use: cuts at the quietest frame boundary between items.
/// Run `f` with the project's loudness, reading the 16 kHz audio once per project.
fn with_loudness<T>(root: &Path, id: &str, f: impl FnOnce(&avskrift_wordalign::Loudness) -> T) -> Result<T> {
    let mut cache = LOUDNESS.lock().map_err(|_| anyhow!("förhandsvisningen behöver startas om"))?;
    if cache.as_ref().is_none_or(|(cached, _)| cached != id) {
        let mut reader = hound::WavReader::open(project_dir(root, id)?.join(AUDIO_FILE))?;
        let rate = reader.spec().sample_rate as usize;
        let samples = reader.samples::<i16>().map_while(|s| s.ok()).map(|s| s as f32 / 32768.0);
        *cache = Some((id.to_string(), avskrift_wordalign::Loudness::from_samples(samples, rate)));
    }
    Ok(f(&cache.as_ref().unwrap().1))
}

/// Waveform bars (0..1) for `[start, end)` seconds of the project's audio.
pub fn waveform(root: &Path, id: &str, start: f64, end: f64, bars: usize) -> Result<Vec<f32>> {
    ensure!(bars <= 20_000 && end >= start, "ogiltigt vågformsintervall");
    with_loudness(root, id, |l| l.waveform(start, end, bars))
}

/// Long quiet stretches (at least `min` seconds) in the whole recording, from the audio alone.
pub fn silences(root: &Path, id: &str, min: f64) -> Result<Vec<(f64, f64)>> {
    ensure!(min > 0.0, "ogiltig längd för tystnader");
    with_loudness(root, id, |l| l.silences(min, 0.15).into_iter().map(|p| (p.start, p.end)).collect())
}

pub fn preview(root: &Path, id: &str, edits: &EditList) -> Result<Preview> {
    let p = load(root, id)?;
    with_loudness(root, id, |loudness| preview_with(&p, edits, loudness))
}

fn preview_with(p: &Project, edits: &EditList, loudness: &avskrift_wordalign::Loudness) -> Preview {
    // Video cuts snap to frames; audio-only projects to milliseconds.
    let fps = p.media.video.as_ref().map_or(1000.0, |v| if v.fps > 0.0 { v.fps } else { 25.0 });
    let keep = avskrift_textklipp::keep_ranges(&items(p), edits, &p.pauses, p.media.duration, |lo, hi| {
        loudness.cut_point(fps, lo, hi)
    });
    Preview { edited_duration: keep.iter().map(|(a, b)| b - a).sum(), keep }
}

// ---------------------------------------------------------------- Export

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportArgs {
    /// Target file chosen by the user (.mp4 for video, .m4a for audio-only projects).
    pub output: String,
    pub quality: avskrift_textklipp::render::Quality,
    #[serde(default)]
    pub srt: bool,
    #[serde(default)]
    pub vtt: bool,
    #[serde(default)]
    pub text: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub output: String,
    /// Extra files written next to the film (subtitles, text).
    pub extra: Vec<String>,
    pub expected_duration: f64,
    pub video_duration: Option<f64>,
    pub audio_duration: f64,
    /// Picture and sound end within one frame of each other, and the length is as planned.
    pub sync_ok: bool,
    pub encoder: Option<String>,
    pub pieces: usize,
    pub seconds: f64,
}

fn stream_durations(tools: &Tools, file: &Path) -> Result<(Option<f64>, f64)> {
    let out = command(&tools.ffprobe)
        .args(["-v", "error", "-show_entries", "stream=codec_type,duration", "-of", "json"])
        .arg(file)
        .output()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let dur = |kind: &str| {
        v["streams"].as_array().and_then(|s| {
            s.iter().find(|x| x["codec_type"] == kind).and_then(|x| x["duration"].as_str()?.parse::<f64>().ok())
        })
    };
    Ok((dur("video"), dur("audio").ok_or_else(|| anyhow!("exporten saknar ljudspår"))?))
}

/// Render the edited film from the original file, in full quality. Uses the saved edit list and
/// the same cut points as the editor's preview. Writes to a temporary name and renames at the end,
/// so a cancel or failure never leaves a half film under the chosen name.
pub fn export(
    paths: &ModelPaths,
    id: &str,
    args: &ExportArgs,
    progress: &dyn Fn(&str),
    pct: &(dyn Fn(i32) + Sync),
) -> Result<ExportResult> {
    use avskrift_textklipp::{render, subtitles};
    let started = std::time::Instant::now();
    let root = &paths.textklipp_dir;
    let tools = paths.ffmpeg_tools();
    let p = load(root, id)?;
    ensure!(p.status == Status::Ready && p.transcript.is_some(), "Projektet är inte färdigimporterat.");
    let src = PathBuf::from(&p.source_path);
    ensure!(src.is_file(), "Originalfilen finns inte längre: {}. Flytta tillbaka den och försök igen.", src.display());
    let output = PathBuf::from(&args.output);
    progress("Räknar fram klippen…");
    let (keep, quiet_at): (Vec<(f64, f64)>, Vec<(f64, bool)>) = with_loudness(root, id, |l| {
        let keep = preview_with(&p, &p.edits, l).keep;
        let quiet = keep.iter().flat_map(|&(a, b)| [(a, l.is_quiet(a)), (b, l.is_quiet(b))]).collect();
        (keep, quiet)
    })?;
    ensure!(!keep.is_empty(), "Allt är bortklippt – det finns inget att exportera.");
    let quiet = |t: f64| quiet_at.iter().find(|q| q.0 == t).is_none_or(|q| q.1);
    let video = match &p.media.video {
        Some(v) => {
            let fps = if v.fps > 0.0 && v.fps <= 120.0 { v.fps } else { 30.0 };
            Some((encoder(&tools)?, args.quality, v.width.min(v.height), fps))
        }
        None => None,
    };
    let fps = video.map(|v| v.3);
    let pieces = render::pieces(&keep, p.media.duration, fps, quiet);
    ensure!(!pieces.is_empty(), "Allt är bortklippt – det finns inget att exportera.");
    let expected: f64 = pieces.iter().map(|x| render::length(x, fps)).sum();
    let tmp = project_dir(root, id)?.join("export-tmp");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)?;
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("mp4").to_string();
    let partial = output.with_extension(format!("avskrift-tmp.{ext}"));
    let result = (|| -> Result<Vec<String>> {
        use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
        let groups: Vec<&[render::Piece]> = render::groups(&pieces, fps).into_iter().map(|r| &pieces[r]).collect();
        let parts: Vec<String> =
            (0..groups.len()).map(|i| tmp.join(format!("part{i:04}.mkv")).to_string_lossy().into_owned()).collect();
        // Groups render in parallel (two FFmpeg processes: roughly twice as fast; consumer NVIDIA
        // cards allow several NVENC sessions). Worker threads have no work context, so they get the
        // request's cancel token explicitly; the first failure stops the other worker too.
        let workers = render::WORKERS.min(groups.len());
        // AVSKRIFT_TEXTKLIPP_DEBUG: keep FFmpeg arguments and intermediate files for diagnosis.
        let debug = std::env::var_os("AVSKRIFT_TEXTKLIPP_DEBUG").is_some();
        progress(&format!("Renderar {} delar, {workers} åt gången…", groups.len()));
        let token = work::token();
        let stop = || token.as_ref().is_some_and(|t| t.load(Relaxed));
        let next = AtomicUsize::new(0);
        let done = std::sync::Mutex::new(vec![0.0f64; groups.len()]);
        let results: std::sync::Mutex<Vec<Option<Result<()>>>> = std::sync::Mutex::new((0..groups.len()).map(|_| None).collect());
        std::thread::scope(|s| {
            for _ in 0..workers {
                s.spawn(|| loop {
                    let i = next.fetch_add(1, Relaxed);
                    if i >= groups.len() || stop() {
                        break;
                    }
                    let len: f64 = groups[i].iter().map(|x| render::length(x, fps)).sum();
                    #[allow(unused_mut)]
                    let mut a = render::group_args(&src.to_string_lossy(), groups[i], video, &parts[i]);
                    #[cfg(test)]
                    if tests::FAIL_ONE_GROUP.swap(false, Relaxed) {
                        a.insert(0, "-this-option-does-not-exist".into()); // simulated encoder failure
                    }
                    if debug {
                        let _ = std::fs::write(format!("{}.args.txt", parts[i]), a.join("\n"));
                    }
                    let report = |q: i32| {
                        let mut d = done.lock().unwrap();
                        d[i] = len * q as f64 / 100.0;
                        pct((d.iter().sum::<f64>() / expected * 90.0) as i32);
                    };
                    // A failed group does not stop the other worker: it is retried alone below.
                    let r = run_ffmpeg_until(&tools, &a, len, &report, &stop);
                    results.lock().unwrap()[i] = Some(r);
                });
            }
        });
        work::check()?;
        // Retry failed groups one at a time: a parallel failure is typically the hardware encoder
        // being unavailable for a moment (e.g. another program holding NVENC sessions).
        let mut results = results.into_inner().unwrap();
        for i in 0..groups.len() {
            if matches!(results[i], Some(Ok(()))) {
                continue;
            }
            if let Some(Err(e)) = &results[i] {
                eprintln!("AVskrift textklipp: del {} misslyckades parallellt ({e}); försöker igen ensam", i + 1);
            }
            progress(&format!("Försöker igen med del {} av {}…", i + 1, groups.len()));
            let len: f64 = groups[i].iter().map(|x| render::length(x, fps)).sum();
            let a = render::group_args(&src.to_string_lossy(), groups[i], video, &parts[i]);
            let base: f64 = done.lock().unwrap().iter().enumerate().filter(|(k, _)| *k != i).map(|(_, d)| d).sum();
            results[i] = Some(run_ffmpeg(&tools, &a, len, &|q| pct(((base + len * q as f64 / 100.0) / expected * 90.0) as i32)));
            if let Some(Err(e)) = &results[i] {
                return Err(anyhow!("{e}"));
            }
        }
        progress("Sätter ihop filmen…");
        let list = tmp.join("parts.txt");
        std::fs::write(&list, render::concat_list(&parts))?;
        let a = render::final_args(&list.to_string_lossy(), video.is_some(), &partial.to_string_lossy());
        run_ffmpeg(&tools, &a, expected, &|q| pct(90 + q * 9 / 100))?;
        if output.exists() {
            std::fs::remove_file(&output)?;
        }
        std::fs::rename(&partial, &output)?;

        let mut extra = Vec::new();
        let t = p.transcript.as_ref().unwrap();
        let words: Vec<(String, f64, f64)> =
            t.utterances.iter().flat_map(|u| u.words.iter().map(|w| (w.text.clone(), w.start, w.end))).collect();
        let cues = subtitles::cues(&words, &keep);
        let mut write = |ext: &str, body: String| -> Result<()> {
            let file = output.with_extension(ext);
            storage::atomic_write(&file, body.as_bytes())?;
            extra.push(file.to_string_lossy().into_owned());
            Ok(())
        };
        if args.srt {
            write("srt", subtitles::srt(&cues))?;
        }
        if args.vtt {
            write("vtt", subtitles::vtt(&cues))?;
        }
        if args.text {
            let paras: Vec<Vec<(String, f64, f64)>> =
                t.utterances.iter().map(|u| u.words.iter().map(|w| (w.text.clone(), w.start, w.end)).collect()).collect();
            write("txt", subtitles::text(&paras, &keep))?;
        }
        Ok(extra)
    })();
    if std::env::var_os("AVSKRIFT_TEXTKLIPP_DEBUG").is_none() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    let extra = match result {
        Ok(extra) => extra,
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            return Err(e);
        }
    };
    progress("Kontrollerar synk…");
    let (video_duration, audio_duration) = stream_durations(&tools, &output)?;
    let frame = video.map_or(0.05, |v| 1.0 / v.3);
    let sync_ok = video_duration.is_none_or(|v| (v - audio_duration).abs() <= frame + 0.03)
        && (audio_duration - expected).abs() <= 0.25;
    pct(100);
    progress("Klar");
    Ok(ExportResult {
        output: output.to_string_lossy().into_owned(),
        extra,
        expected_duration: expected,
        video_duration,
        audio_duration,
        sync_ok,
        encoder: video.map(|v| v.0.to_string()),
        pieces: pieces.len(),
        seconds: started.elapsed().as_secs_f64(),
    })
}

// ---------------------------------------------------------------- Projects

pub fn project_dir(root: &Path, id: &str) -> Result<PathBuf> {
    ensure!(!id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'), "ogiltigt projekt-id");
    Ok(root.join(id))
}

pub fn save(root: &Path, project: &Project) -> Result<()> {
    let dir = project_dir(root, &project.id)?;
    std::fs::create_dir_all(&dir)?;
    storage::atomic_write(&dir.join(PROJECT_FILE), &serde_json::to_vec_pretty(project)?)
}

pub fn load(root: &Path, id: &str) -> Result<Project> {
    let path = project_dir(root, id)?.join(PROJECT_FILE);
    let bytes = std::fs::read(&path).with_context(|| format!("projektet {id} finns inte"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn list(root: &Path) -> Vec<ProjectMeta> {
    let mut out: Vec<ProjectMeta> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| load(root, &e.file_name().to_string_lossy()).ok())
        .map(|p| ProjectMeta {
            id: p.id,
            title: p.title,
            updated_at: p.updated_at,
            duration: p.media.duration,
            status: p.status,
            has_proxy: p.proxy.is_some(),
        })
        .collect();
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

pub fn delete(root: &Path, id: &str) -> Result<()> {
    let dir = project_dir(root, id)?;
    ensure!(dir.join(PROJECT_FILE).is_file(), "projektet {id} finns inte");
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

/// Apply alignment output to a project: word times are already in the transcript; sound blocks
/// get ids above all word ids.
pub fn set_alignment(p: &mut Project, refined: &crate::wordalign::Refined) {
    p.sounds = refined
        .sounds
        .iter()
        .enumerate()
        .map(|(i, s)| Sound {
            id: SOUND_ID_BASE + i as u32,
            start: s.start,
            end: s.end,
            heard: s.heard.clone(),
            score: s.score,
        })
        .collect();
    p.pauses = refined.pauses.iter().map(|q| (q.start, q.end)).collect();
    p.word_times = "exakta".into();
}

pub fn now() -> String {
    // RFC 3339 in UTC from the system clock; sortable like the frontend's ISO strings.
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil-from-days (Howard Hinnant), valid for the Gregorian calendar.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn new_id() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("tk-{nanos:x}")
}

pub fn title_of(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Namnlöst klipp".into())
}

pub fn check_source(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("Filen finns inte: {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_titles() {
        assert!(project_dir(Path::new("x"), "tk-1a2b").is_ok());
        assert!(project_dir(Path::new("x"), "../evil").is_err());
        assert!(project_dir(Path::new("x"), "").is_err());
        assert_eq!(title_of(Path::new(r"C:\a\test till joels descript.mp4")), "test till joels descript");
        let n = now();
        assert_eq!(n.len(), 20);
        assert!(n.starts_with("20") && n.ends_with('Z'));
    }

    /// Makes the next rendered group fail once (see `export_repeat`).
    pub(super) static FAIL_ONE_GROUP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    fn bundled(exe: &str) -> PathBuf {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("ffmpeg").join(exe);
        if p.is_file() {
            p
        } else {
            exe.into()
        }
    }

    /// Paths for opt-in tests: models from env, projects in a scratch dir.
    fn test_paths(root: &Path) -> ModelPaths {
        let env = |k: &str| PathBuf::from(std::env::var(k).unwrap_or_default());
        let whisper = env("AVSKRIFT_TEXTKLIPP_TEST_WHISPER");
        ModelPaths {
            whisper_dir: whisper.parent().unwrap_or(Path::new("")).to_path_buf(),
            pianissimo_dir: root.join("none"),
            wordalign_dir: env("AVSKRIFT_WORDALIGN_TEST_MODEL"),
            diar_segmentation: root.join("none"),
            diar_embedding: root.join("none"),
            ner_model: root.join("none"),
            ner_tokenizer: root.join("none"),
            ner_labels: root.join("none"),
            llm_model: root.join("none"),
            llm_tokenizer: root.join("none"),
            summary_dir: root.join("none"),
            jobs_dir: root.join("none"),
            meetings_dir: root.join("none"),
            tasks_file: root.join("none"),
            textklipp_dir: root.to_path_buf(),
            // The bundled LGPL build when fetched (model-tools/fetch-ffmpeg.ps1), else PATH.
            ffmpeg: bundled("ffmpeg.exe"),
            ffprobe: bundled("ffprobe.exe"),
        }
    }

    /// Opt-in end-to-end import of a real recording: AVSKRIFT_TEXTKLIPP_TEST_VIDEO,
    /// AVSKRIFT_TEXTKLIPP_TEST_WHISPER (a KB-Whisper .bin), AVSKRIFT_WORDALIGN_TEST_MODEL.
    #[test]
    #[ignore]
    fn import_real_video() {
        let root = std::env::temp_dir().join(format!("avskrift-tk-e2e-{}", new_id()));
        let paths = test_paths(&root);
        let whisper = PathBuf::from(std::env::var("AVSKRIFT_TEXTKLIPP_TEST_WHISPER").unwrap());
        let model = whisper.file_stem().unwrap().to_string_lossy().into_owned();
        let args = ImportArgs {
            path: std::env::var("AVSKRIFT_TEXTKLIPP_TEST_VIDEO").unwrap(),
            model,
            language: "sv".into(),
            diarize: false,
            num_speakers: None,
        };
        let last = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(-1));
        let seen = last.clone();
        let pct: Pct = std::sync::Arc::new(move |p| {
            let before = seen.swap(p, std::sync::atomic::Ordering::Relaxed);
            assert!(p >= before.min(p), "progress went backwards");
        });
        let started = std::time::Instant::now();
        let p = import(&paths, &args, &|m| println!("{:7.1}s {m}", started.elapsed().as_secs_f64()), &pct).unwrap();
        let words: usize = p.transcript.as_ref().unwrap().utterances.iter().map(|u| u.words.len()).sum();
        let dir = project_dir(&root, &p.id).unwrap();
        println!(
            "IMPORT {:.1}s: {} words, word_times={}, {} sounds, {} pauses, proxy={:?}, error={:?}",
            started.elapsed().as_secs_f64(),
            words,
            p.word_times,
            p.sounds.len(),
            p.pauses.len(),
            p.proxy,
            p.error
        );
        assert_eq!(p.status, Status::Ready);
        assert_eq!(p.word_times, "exakta");
        assert!(words > 0 && dir.join(AUDIO_FILE).is_file());
        if p.media.video.is_some() {
            assert!(dir.join(PROXY_FILE).is_file());
        }
        assert_eq!(load(&root, &p.id).unwrap().transcript.unwrap().utterances.len(), p.transcript.unwrap().utterances.len());
        if std::env::var("AVSKRIFT_TEXTKLIPP_KEEP").is_err() {
            let _ = std::fs::remove_dir_all(&root);
        } else {
            println!("kept {}", dir.display());
        }
    }

    /// Opt-in end-to-end export (same env as `import_real_video`; AVSKRIFT_TEXTKLIPP_EXPORT_DIR
    /// receives the film). Strikes the first two words, every other sentence's first word, and
    /// shortens pauses, then checks sync, length and that struck words are not in the subtitles.
    #[test]
    #[ignore]
    fn export_real_video() {
        let root = std::env::temp_dir().join(format!("avskrift-tk-export-{}", new_id()));
        let paths = test_paths(&root);
        let whisper = PathBuf::from(std::env::var("AVSKRIFT_TEXTKLIPP_TEST_WHISPER").unwrap());
        let args = ImportArgs {
            path: std::env::var("AVSKRIFT_TEXTKLIPP_TEST_VIDEO").unwrap(),
            model: whisper.file_stem().unwrap().to_string_lossy().into_owned(),
            language: "sv".into(),
            diarize: false,
            num_speakers: None,
        };
        let pct: Pct = std::sync::Arc::new(|_| {});
        let mut p = import(&paths, &args, &|_| {}, &pct).unwrap();
        let words: Vec<(u32, String)> = p
            .transcript
            .as_ref()
            .unwrap()
            .utterances
            .iter()
            .flat_map(|u| u.words.iter().map(|w| w.text.clone()))
            .enumerate()
            .map(|(i, w)| (i as u32, w))
            .collect();
        // Strike the first two words and the first word after every sentence end.
        let mut struck: Vec<u32> = vec![0, 1];
        struck.extend(words.windows(2).filter(|w| w[0].1.ends_with('.')).map(|w| w[1].0));
        p.edits.deleted = struck.iter().copied().collect();
        p.edits.pause_limit = Some(0.7);
        save(&root, &p).unwrap();
        let out_dir = PathBuf::from(std::env::var("AVSKRIFT_TEXTKLIPP_EXPORT_DIR").unwrap());
        std::fs::create_dir_all(&out_dir).unwrap();
        let output = out_dir.join("export-test.mp4");
        // The keep ranges, for an external sync check against the original audio.
        // The pieces exactly as exported (snapped to frames), for an external sync check.
        let fps = p.media.video.as_ref().map(|v| v.fps);
        let keep: Vec<(f64, f64)> = avskrift_textklipp::render::pieces(&preview(&root, &p.id, &p.edits).unwrap().keep, p.media.duration, fps, |_| true)
            .iter()
            .map(|x| (x.start, x.start + avskrift_textklipp::render::length(x, fps)))
            .collect();
        std::fs::write(out_dir.join("export-test.keep.json"), serde_json::to_string(&keep).unwrap()).unwrap();
        std::fs::copy(project_dir(&root, &p.id).unwrap().join(AUDIO_FILE), out_dir.join("export-test.source16k.wav")).unwrap();
        let ex = ExportArgs { output: output.to_string_lossy().into(), quality: avskrift_textklipp::render::Quality::High, srt: true, vtt: true, text: true };
        let started = std::time::Instant::now();
        let r = export(&paths, &p.id, &ex, &|m| println!("{:6.1}s {m}", started.elapsed().as_secs_f64()), &|_| {}).unwrap();
        println!(
            "EXPORT {:.1}s: {} pieces, expected {:.3}s, video {:?}, audio {:.3}, sync_ok={}, encoder={:?}",
            r.seconds, r.pieces, r.expected_duration, r.video_duration, r.audio_duration, r.sync_ok, r.encoder
        );
        assert!(r.sync_ok);
        assert!(output.is_file() && r.extra.len() == 3);
        let srt = std::fs::read_to_string(output.with_extension("srt")).unwrap();
        let text = std::fs::read_to_string(output.with_extension("txt")).unwrap();
        assert!(!srt.is_empty() && text.split_whitespace().count() < words.len());
        assert!(!out_dir.read_dir().unwrap().flatten().any(|e| e.file_name().to_string_lossy().contains("avskrift-tmp")));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Opt-in stress test: export an already imported project (AVSKRIFT_TEXTKLIPP_TEST_ROOT = the
    /// projects folder, AVSKRIFT_TEXTKLIPP_TEST_ID) AVSKRIFT_TEXTKLIPP_REPEAT times; FFmpeg's full
    /// error output is printed for any failure.
    #[test]
    #[ignore]
    fn export_repeat() {
        let root = PathBuf::from(std::env::var("AVSKRIFT_TEXTKLIPP_TEST_ROOT").unwrap());
        let id = std::env::var("AVSKRIFT_TEXTKLIPP_TEST_ID").unwrap();
        let times: usize = std::env::var("AVSKRIFT_TEXTKLIPP_REPEAT").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
        let paths = test_paths(&root);
        // AVSKRIFT_TEXTKLIPP_INJECT_FAIL: the first group of the first export fails once.
        if std::env::var_os("AVSKRIFT_TEXTKLIPP_INJECT_FAIL").is_some() {
            FAIL_ONE_GROUP.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let out = std::env::temp_dir().join(format!("avskrift-tk-repeat-{}.mp4", new_id()));
        let ex = ExportArgs { output: out.to_string_lossy().into(), quality: avskrift_textklipp::render::Quality::High, srt: false, vtt: false, text: false };
        let mut failures = 0;
        for i in 0..times {
            match export(&paths, &id, &ex, &|_| {}, &|_| {}) {
                Ok(r) => println!("REPEAT {i}: ok {:.1}s sync_ok={}", r.seconds, r.sync_ok),
                Err(e) => {
                    failures += 1;
                    println!("REPEAT {i}: FAIL {e}");
                }
            }
        }
        let _ = std::fs::remove_file(&out);
        println!("REPEAT {failures} failures of {times}");
        assert_eq!(failures, 0);
    }

    /// Opt-in: cancelling during the proxy stops FFmpeg promptly and leaves no partial file.
    #[test]
    #[ignore]
    fn proxy_cancel_stops_ffmpeg() {
        let root = std::env::temp_dir().join(format!("avskrift-tk-cancel-{}", new_id()));
        let paths = test_paths(&root);
        let src = PathBuf::from(std::env::var("AVSKRIFT_TEXTKLIPP_TEST_VIDEO").unwrap());
        let media = probe(&paths.ffmpeg_tools(), &src).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        let dst = root.join(PROXY_FILE);
        let id = work::begin().unwrap();
        let cancel_id = id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            assert!(work::cancel(&cancel_id));
        });
        let started = std::time::Instant::now();
        let r = work::run(Some(id), || make_proxy(&paths.ffmpeg_tools(), &src, &dst, &media, &|_| {}));
        assert!(r.unwrap_err().is::<work::Cancelled>());
        assert!(started.elapsed().as_secs() < 6, "took {:?}", started.elapsed());
        assert!(!dst.is_file() && !dst.with_extension("partial.mp4").is_file());
        println!("CANCEL proxy stopped after {:.1}s", started.elapsed().as_secs_f64());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn project_round_trip_and_list() {
        let root = std::env::temp_dir().join(format!("avskrift-tk-test-{}", new_id()));
        let media = MediaInfo { duration: 12.0, size_bytes: 1, container: "mov".into(), video: None, audio: None };
        let mut p = Project {
            version: 1,
            id: new_id(),
            title: "Test".into(),
            created_at: now(),
            updated_at: now(),
            source_path: "x.mp4".into(),
            media,
            status: Status::Ready,
            error: None,
            transcript: None,
            word_times: "exakta".into(),
            sounds: vec![],
            pauses: vec![(1.0, 2.0)],
            proxy: None,
            edits: EditList::default(),
        };
        p.edits.deleted.insert(3);
        save(&root, &p).unwrap();
        let back = load(&root, &p.id).unwrap();
        assert_eq!(back.edits, p.edits);
        assert_eq!(back.pauses, p.pauses);
        assert_eq!(list(&root).len(), 1);
        delete(&root, &p.id).unwrap();
        assert!(list(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
