//! Speech-to-text via whisper.cpp (`whisper-rs`) using KB-Whisper GGML models.
//!
//! The context is loaded lazily and kept across calls; switching model id reloads it. Output is a
//! flat list of timed segments (~sentence/phrase level), each optionally carrying word-level
//! timestamps, which `align` then attributes to speakers.

use crate::memory::{self, Cache, DeviceFailure};
use once_cell::sync::Lazy;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState};

/// One word with absolute timestamps in seconds.
#[derive(Debug, Clone)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// One recognised segment with absolute timestamps in seconds.
#[derive(Debug, Clone)]
pub struct RawSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    /// Word-level timing, present only when requested.
    pub words: Vec<Word>,
}

pub struct Transcriber {
    // Native model/state live in the shared, expiring SPEECH cache below.
    threads: usize,
    use_gpu: bool,
}

impl Transcriber {
    pub fn new() -> Self {
        Transcriber {
            threads: num_cpus::get_physical().clamp(1, 8),
            use_gpu: cfg!(any(feature = "cuda", feature = "metal", feature = "vulkan")),
        }
    }

    fn ensure(cache: &mut Cache<LoadedSpeech>, path: &Path, allow_gpu: bool) -> Result<()> {
        if cache.value.as_ref().is_none_or(|m| m.path != path || m.allow_gpu != allow_gpu) {
            cache.clear();
            cache.value = Some(LoadedSpeech::load(path, allow_gpu, false)?);
        }
        Ok(())
    }
    /// Warm compute buffers while the user is speaking; the cache stays warm for two minutes.
    #[cfg(test)]
    pub fn prepare(&mut self, _id: &str, path: &Path) -> Result<()> {
        let path = std::fs::canonicalize(path)?;
        let _work = memory::enter()?;
        let mut cache = SPEECH.lock().map_err(|_| anyhow!("Talmodellens cache behöver startas om."))?;
        Self::ensure(&mut cache, &path, self.use_gpu)?;
        crate::work::check()?;
        cache.touch();
        Ok(())
    }

    pub fn try_prepare(&mut self, path: &Path) -> Result<bool> {
        let Ok(_work) = memory::WORK.try_lock() else { return Ok(false); };
        let path = std::fs::canonicalize(path)?;
        let mut cache = SPEECH.lock().map_err(|_| anyhow!("Talmodellens cache behöver startas om."))?;
        Self::ensure(&mut cache, &path, self.use_gpu)?;
        crate::work::check()?;
        cache.touch(); Ok(true)
    }

    pub fn backend_label() -> &'static str {
        if cfg!(feature = "cuda") {
            "CUDA – GPU när tillgänglig"
        } else if cfg!(feature = "vulkan") {
            "Vulkan – GPU när tillgänglig"
        } else if cfg!(feature = "metal") {
            "Metal – GPU när tillgänglig"
        } else {
            "CPU – det här bygget saknar GPU-stöd"
        }
    }

    /// Transcribe 16 kHz mono f32 `samples`. `language` is an ISO code, or "auto" to detect.
    /// When `word_timestamps` is set, each segment is filled with word-level timing. When
    /// `translate` is set, Whisper translates the speech to English. `pct` receives 0–100 progress.
    #[allow(clippy::too_many_arguments)]
    pub fn transcribe(
        &mut self,
        id: &str,
        path: &Path,
        samples: &[f32],
        language: &str,
        word_timestamps: bool,
        translate: bool,
        progress: &dyn Fn(&str),
        pct: impl Fn(i32) + Send + Sync + 'static,
    ) -> Result<Vec<RawSegment>> {
        let path = std::fs::canonicalize(path).with_context(|| {
            format!("Whisper-modellen '{id}' är inte tillgänglig. Hämta den under Modeller på datorn.")
        })?;
        progress("Förbereder transkribering…");
        let _work = memory::enter()?;
        let mut cache = SPEECH.lock().map_err(|_| anyhow!("Talmodellens cache behöver startas om."))?;
        Self::ensure(&mut cache, &path, self.use_gpu)?;
        crate::work::check()?;
        let pct: Arc<dyn Fn(i32) + Send + Sync> = Arc::new(pct);
        let run = |m: &mut LoadedSpeech| {
            progress(&format!("Transkriberar – {}…", m.mode));
            transcribe_state(&mut m.state, self.threads, samples, language, word_timestamps, translate, pct.clone())
        };
        let result = run(cache.value.as_mut().unwrap());
        let result = match result {
            Err(e) if memory::can_retry(cache.value.as_ref().unwrap().gpu, &e) => {
                crate::work::check()?;
                cache.clear();
                crate::llm::release_cached();
                progress("GPU-körningen misslyckades. Försöker igen på CPU…");
                pct(0);
                cache.value = Some(LoadedSpeech::load(&path, self.use_gpu, true).map_err(|cpu| {
                    anyhow!("GPU-körningen misslyckades ({e}). CPU-reservvägen kunde inte starta: {cpu}")
                })?);
                run(cache.value.as_mut().unwrap())
                    .map_err(|cpu| anyhow!("GPU-körningen misslyckades ({e}). Även CPU-försöket misslyckades: {cpu}"))
            }
            other => other,
        };
        if result.as_ref().err().is_some_and(|e| e.is::<crate::work::Cancelled>()) { cache.clear(); }
        cache.touch();
        result
    }
}

#[allow(clippy::too_many_arguments)]
fn transcribe_state(
    state: &mut WhisperState,
    threads: usize,
    samples: &[f32],
    language: &str,
    word_timestamps: bool,
    translate: bool,
    pct: Arc<dyn Fn(i32) + Send + Sync>,
) -> Result<Vec<RawSegment>> {
    #[cfg(test)]
    if memory::take_fault(memory::Fault::SpeechCompute) {
        return Err(anyhow!(DeviceFailure("syntetiskt GPU-beräkningsfel".into())));
    }
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    if language != "auto" {
        params.set_language(Some(language));
    }
    params.set_translate(translate);
    // Keep each recording independent despite reusing the native allocation.
    params.set_no_context(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_token_timestamps(word_timestamps);
    // Live percent progress from whisper.cpp (0–100). Verify this API name against the pinned
    // whisper-rs version (see FINISH.md); older versions use `set_progress_callback`.
    params.set_progress_callback_safe(move |p: i32| pct(p));
    // Keep synchronization overhead bounded, especially on hybrid P/E-core processors.
    params.set_n_threads(threads as i32);

    crate::work::check()?;
    // The Arc outlives the synchronous native call. Callback only reads an atomic, never Rust
    // model state. Avoid the pinned wrapper's safe callback which boxes/casts closures incorrectly.
    let cancel = crate::work::token();
    if let Some(token) = &cancel {
        unsafe {
            params.set_abort_callback(Some(abort_requested));
            params.set_abort_callback_user_data(Arc::as_ptr(token) as *mut std::ffi::c_void);
        }
    }
    let result = state.full(params, samples);
    crate::work::check()?; // A requested abort is not a GPU failure and must never trigger CPU retry.
    result.map_err(speech_error)?;

    let n = state.full_n_segments().map_err(|e| anyhow!("kunde inte läsa segment: {e}"))?;

    // Whisper's BPE tokens are byte-level, so a multi-byte UTF-8 character (e.g. å/ä/ö) can be
    // split across a token — and thus a segment — boundary. The strict text accessors fail on
    // such segments ("Invalid UTF-8 detected"). Fetch raw bytes instead, reunite dangling
    // continuation bytes with their lead byte in the previous segment, and decode lossily as a
    // last resort.
    let mut raw: Vec<Vec<u8>> = Vec::with_capacity(n as usize);
    for i in 0..n {
        raw.push(state.full_get_segment_bytes(i).map_err(|e| anyhow!("kunde inte läsa segmenttext: {e}"))?);
    }
    for i in 1..raw.len() {
        let missing = utf8_missing_continuation(&raw[i - 1]);
        if missing > 0 {
            let take = raw[i].iter().take(missing).take_while(|&&b| is_utf8_continuation(b)).count();
            let moved: Vec<u8> = raw[i].drain(..take).collect();
            raw[i - 1].extend_from_slice(&moved);
        }
    }

    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let text = String::from_utf8_lossy(&raw[i as usize]).trim().to_string();
        let t0 = state.full_get_segment_t0(i).map_err(|e| anyhow!("tidsfel: {e}"))?;
        let t1 = state.full_get_segment_t1(i).map_err(|e| anyhow!("tidsfel: {e}"))?;
        if text.is_empty() {
            continue;
        }
        let words = if word_timestamps {
            // Group whisper's sub-word tokens into words. A new word begins at a token whose
            // text starts with a space; special/marker tokens ("[_…]") are skipped. A character
            // can be split across tokens here too, so bytes are accumulated per word and only
            // decoded once the word is complete.
            let mut ws: Vec<(f64, f64, Vec<u8>)> = Vec::new();
            let n_tokens = state.full_n_tokens(i).unwrap_or(0);
            for j in 0..n_tokens {
                let tok = match state.full_get_token_bytes(i, j) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                if tok.starts_with(b"[_") {
                    continue;
                }
                let data = match state.full_get_token_data(i, j) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let (start, end) = (data.t0 as f64 / 100.0, data.t1 as f64 / 100.0);
                // A continuation byte never starts a word: it completes a character whose
                // lead byte sits in the previous token.
                let continues_char = tok.first().copied().is_some_and(is_utf8_continuation);
                let starts_word = (tok.first() == Some(&b' ') || ws.is_empty()) && !continues_char;
                if starts_word {
                    let piece: Vec<u8> = tok.iter().copied().skip_while(|b| b.is_ascii_whitespace()).collect();
                    if piece.is_empty() {
                        continue;
                    }
                    ws.push((start, end, piece));
                } else if let Some(last) = ws.last_mut() {
                    last.2.extend_from_slice(&tok);
                    last.1 = end;
                }
            }
            ws.into_iter()
                .map(|(start, end, bytes)| Word { start, end, text: String::from_utf8_lossy(&bytes).into_owned() })
                .collect()
        } else {
            Vec::new()
        };
        // whisper timestamps are in centiseconds (10 ms units).
        out.push(RawSegment { start: t0 as f64 / 100.0, end: t1 as f64 / 100.0, text, words });
    }
    Ok(out)
}

unsafe extern "C" fn abort_requested(data: *mut std::ffi::c_void) -> bool {
    // Valid only during state.full; ownership stays with its local Arc.
    unsafe { &*(data as *const std::sync::atomic::AtomicBool) }.load(std::sync::atomic::Ordering::Relaxed)
}

fn is_utf8_continuation(b: u8) -> bool {
    matches!(b, 0x80..=0xBF)
}

/// How many continuation bytes the (possibly incomplete) UTF-8 sequence at the end of `bytes`
/// still needs. 0 when the tail is complete, or is not the start of a multi-byte sequence at all.
fn utf8_missing_continuation(bytes: &[u8]) -> usize {
    // The lead byte of an incomplete sequence sits at most 3 positions from the end
    // (a 4-byte sequence missing only its last byte).
    for back in 1..=bytes.len().min(3) {
        let b = bytes[bytes.len() - back];
        if is_utf8_continuation(b) {
            continue;
        }
        let need: usize = match b {
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return 0, // ASCII or an invalid lead byte — nothing to complete
        };
        return need.saturating_sub(back);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::utf8_missing_continuation;

    /// Opt-in measurement with a local fixture; never prints the recognized text.
    /// AVSKRIFT_BENCH_MODEL and AVSKRIFT_BENCH_AUDIO must name existing local files.
    #[test]
    #[ignore]
    fn benchmark_transcription() {
        let model = std::env::var("AVSKRIFT_BENCH_MODEL").expect("AVSKRIFT_BENCH_MODEL");
        let input = std::env::var("AVSKRIFT_BENCH_AUDIO").expect("AVSKRIFT_BENCH_AUDIO");
        let audio = crate::audio::load(std::path::Path::new(&input)).unwrap();
        let mut transcriber = super::Transcriber::new();
        if let Ok(threads) = std::env::var("AVSKRIFT_BENCH_THREADS") {
            transcriber.threads = threads.parse().unwrap();
        }
        if std::env::var("AVSKRIFT_BENCH_GPU").as_deref() == Ok("0") {
            transcriber.use_gpu = false;
        }
        for run in 0..3 {
            let start = std::time::Instant::now();
            let result = transcriber
                .transcribe(
                    "benchmark",
                    std::path::Path::new(&model),
                    &audio.samples,
                    "sv",
                    false,
                    false,
                    &|_| {},
                    |_| {},
                )
                .unwrap();
            assert!(!result.is_empty());
            println!(
                "BENCH run={run} audio_seconds={:.2} elapsed_ms={} characters={}",
                audio.duration_s,
                start.elapsed().as_millis(),
                result.iter().map(|s| s.text.chars().count()).sum::<usize>()
            );
        }
    }

    #[test]
    #[ignore]
    fn cached_state_keeps_recordings_independent() {
        let model = std::env::var("AVSKRIFT_BENCH_MODEL").expect("AVSKRIFT_BENCH_MODEL");
        let input = std::env::var("AVSKRIFT_BENCH_AUDIO").expect("AVSKRIFT_BENCH_AUDIO");
        let audio = crate::audio::load(std::path::Path::new(&input)).unwrap();
        let mut transcriber = super::Transcriber::new();
        let mut run = |samples: &[f32], words| {
            transcriber
                .transcribe("fixture", std::path::Path::new(&model), samples, "sv", words, false, &|_| {}, |_| {})
                .unwrap()
        };
        let first = run(&audio.samples, false).into_iter().map(|s| s.text).collect::<Vec<_>>();
        assert!(!first.is_empty());
        // Change both audio and word-timestamp settings between identical runs.
        let _ = run(&audio.samples[audio.samples.len() / 2..], true);
        let last = run(&audio.samples, false).into_iter().map(|s| s.text).collect::<Vec<_>>();
        assert_eq!(first, last);
    }

    #[test]
    fn complete_tails_need_nothing() {
        assert_eq!(utf8_missing_continuation(b""), 0);
        assert_eq!(utf8_missing_continuation(b"hej"), 0);
        assert_eq!(utf8_missing_continuation("hör".as_bytes()), 0);
        assert_eq!(utf8_missing_continuation("h€".as_bytes()), 0);
        assert_eq!(utf8_missing_continuation("h😀".as_bytes()), 0);
    }

    #[test]
    fn split_characters_report_missing_bytes() {
        // "hö" cut after the lead byte of ö (0xC3 0xB6).
        assert_eq!(utf8_missing_continuation(&[b'h', 0xC3]), 1);
        // "€" (0xE2 0x82 0xAC) cut after one and two bytes.
        assert_eq!(utf8_missing_continuation(&[0xE2]), 2);
        assert_eq!(utf8_missing_continuation(&[0xE2, 0x82]), 1);
        // "😀" (0xF0 0x9F 0x98 0x80) cut after three bytes.
        assert_eq!(utf8_missing_continuation(&[0xF0, 0x9F, 0x98]), 1);
    }

    #[test]
    fn garbage_tails_are_left_alone() {
        // Continuation bytes with no lead byte in reach.
        assert_eq!(utf8_missing_continuation(&[0x80, 0x80, 0x80, 0x80]), 0);
        // Invalid lead byte.
        assert_eq!(utf8_missing_continuation(&[b'h', 0xFF]), 0);
    }
}

struct LoadedSpeech {
    path: PathBuf,
    state: WhisperState,
    allow_gpu: bool,
    gpu: bool,
    mode: String,
}
static SPEECH: Lazy<Mutex<Cache<LoadedSpeech>>> = Lazy::new(|| Mutex::new(Cache::new()));
pub(crate) fn release_cached() {
    if let Ok(mut c) = SPEECH.try_lock() {
        c.clear();
    }
}
pub(crate) fn sweep(now: std::time::Instant, pressure: bool) {
    if let Ok(mut c) = SPEECH.try_lock() {
        c.sweep(now, pressure);
    }
}
pub(crate) fn cache_status() -> Option<String> {
    SPEECH.try_lock().ok()?.value.as_ref().map(|m| m.mode.clone())
}
fn speech_error(e: whisper_rs::WhisperError) -> anyhow::Error {
    use whisper_rs::WhisperError::*;
    match e {
        InitError
        | FailedToCreateState
        | UnableToCalculateEvaluation
        | FailedToEncode
        | FailedToDecode
        | GenericError(_) => anyhow!(DeviceFailure(format!("transkriberingen misslyckades: {e}"))),
        _ => anyhow!("transkriberingen misslyckades: {e}"),
    }
}
impl LoadedSpeech {
    fn load(path: &Path, allow_gpu: bool, force_cpu: bool) -> Result<Self> {
        // Includes persistent Whisper state and compute buffers. This is a conservative estimate,
        // not a native per-model allocation measurement as used by llama's fit API.
        let bytes = std::fs::metadata(path)?.len().saturating_mul(2).saturating_add(512 * memory::MIB);
        let mut info = memory::sample();
        if info.ram_pressure() || (allow_gpu && !info.speech_gpu_fits(bytes)) || !info.cpu_fits(bytes) {
            crate::llm::release_cached();
            info = memory::sample();
        }
        let gpu = allow_gpu && !force_cpu && info.speech_gpu_fits(bytes);
        if !gpu && !info.cpu_fits(bytes) {
            return Err(anyhow!("För lite ledigt arbetsminne för talmodellen. Stäng andra program eller välj en mindre talmodell under Modeller på datorn."));
        }
        let mut params = WhisperContextParameters::default();
        params.use_gpu(gpu);
        let load = || -> Result<WhisperState> {
            let ctx =
                WhisperContext::new_with_params(path.to_str().ok_or_else(|| anyhow!("ogiltig modellsökväg"))?, params)
                    .map_err(speech_error)?;
            ctx.create_state().map_err(speech_error)
        };
        let state = match load() {
            Ok(state) => state,
            Err(e) if memory::can_retry(gpu, &e) => {
                crate::work::check()?;
                return Self::load(path, allow_gpu, true)
                    .map_err(|cpu| anyhow!("Talmodellen kunde inte laddas på GPU ({e}). CPU: {cpu}"))
            }
            Err(e) => return Err(e),
        };
        let mode = if gpu {
            "GPU"
        } else if force_cpu {
            "CPU-reservväg efter GPU-fel"
        } else {
            "CPU, anpassat efter tillgängligt minne"
        }
        .to_string();
        eprintln!("AVskrift talmodell: {mode}");
        Ok(Self { path: path.to_path_buf(), state, allow_gpu, gpu, mode })
    }
}

#[cfg(test)]
mod memory_native_tests {
    use super::*;
    #[test]
    #[ignore]
    fn cancellation_native_speech_then_fresh_recording() {
        let model = std::env::var("AVSKRIFT_BENCH_MODEL").unwrap();
        let path = Path::new(&model);
        let mut t = Transcriber::new(); t.prepare("cancel-test",path).unwrap();
        let samples: Vec<f32> = (0..480_000).map(|i| (i as f32 * 0.07).sin() * 0.1).collect();
        let id = crate::work::begin().unwrap(); let cancel_id = id.clone();
        let cancel = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(crate::work::cancel(&cancel_id));
        });
        let start = std::time::Instant::now();
        let result = crate::work::run(Some(id), || t.transcribe("cancel-test",path,&samples,"sv",false,false,&|_|{},|_|{}));
        cancel.join().unwrap(); assert!(result.unwrap_err().is::<crate::work::Cancelled>());
        assert!(start.elapsed().as_secs() < 20);
        assert!(SPEECH.lock().unwrap().value.is_none(), "aborted state must be discarded, without CPU retry");
        t.transcribe("cancel-test",path,&samples[..32000],"sv",false,false,&|_|{},|_|{}).unwrap();
        println!("CANCEL speech: native abort, discarded state, fresh recording passed");
    }
    #[test]
    #[ignore]
    fn speech_cache_and_cpu_recovery_with_real_model() {
        let path = std::env::var("AVSKRIFT_BENCH_MODEL").expect("existing Whisper model");
        let path = Path::new(&path);
        let mut t = Transcriber::new();
        t.prepare("memory-test", path).unwrap();
        let gpu = SPEECH.lock().unwrap().value.as_ref().unwrap().gpu;
        // Synthetic low-amplitude tone, never user audio; compare timing/text to a fresh CPU pass.
        let samples: Vec<f32> = (0..32000).map(|i| ((i as f32) * 0.07).sin() * 0.0001).collect();
        if gpu {
            memory::inject(memory::Fault::SpeechCompute);
        }
        let result = t.transcribe("memory-test", path, &samples, "sv", true, false, &|_| {}, |_| {}).unwrap();
        assert!(!SPEECH.lock().unwrap().value.as_ref().unwrap().gpu);
        let result: Vec<_> = result.into_iter().map(|s| (s.start, s.end, s.text)).collect();
        {
            let _work = memory::enter().unwrap();
            sweep(std::time::Instant::now() + memory::IDLE + std::time::Duration::from_secs(1), false);
            assert!(cache_status().is_none());
        }
        t.use_gpu = false;
        let baseline = t.transcribe("memory-test", path, &samples, "sv", true, false, &|_| {}, |_| {}).unwrap();
        assert_eq!(result, baseline.into_iter().map(|s| (s.start, s.end, s.text)).collect::<Vec<_>>());
        println!("MEMORY speech gpu={gpu} recovery/expiry/reload equals fresh CPU result");
    }
}
