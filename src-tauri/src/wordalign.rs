//! Precise word times after transcription (Textklipp, docs/TEXTKLIPP-FAS1.md).
//! Lock order matches Whisper: WORK, then model cache. GPU via DirectML first, CPU as fallback.
use crate::{
    memory::{self, Cache},
    transcript::Utterance,
    work,
};
use anyhow::{anyhow, ensure, Result};
use avskrift_wordalign as native;
use once_cell::sync::Lazy;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Where the exported model is downloaded from (`{SOURCE}/{file}`): a dedicated, immutable release
/// of this repository (model-tools/export-voxrex.py output). Files are verified against
/// `avskrift_wordalign::FILES`. `None` would disable installation with a clear message.
pub const SOURCE: Option<&str> = Some("https://github.com/Pluggentipsar/avskrift/releases/download/models-wordalign-1");

/// Host memory needed to run the model on CPU (fp16 weights are expanded to fp32 there).
const CPU_BYTES: u64 = 2 * 1024 * memory::MIB;

struct Loaded {
    dir: PathBuf,
    emitter: native::Emitter,
    vocab: native::Vocab,
}
static CACHE: Lazy<Mutex<Cache<Loaded>>> = Lazy::new(|| Mutex::new(Cache::new()));
static INSTALL: Mutex<()> = Mutex::new(());

/// What alignment found besides word times; kept for the Textklipp editor.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Refined {
    pub sounds: Vec<native::SoundBlock>,
    pub pauses: Vec<native::Pause>,
    pub device: &'static str,
}

pub fn ready(dir: &Path) -> bool {
    native::FILES.iter().all(|(file, _)| dir.join(file).is_file())
}

pub fn install(dir: &Path, progress: &dyn Fn(&str), bytes: &dyn Fn(u64, u64)) -> Result<()> {
    let base = SOURCE.ok_or_else(|| anyhow!("Modellen för exakta ordtider är inte publicerad ännu."))?;
    let _install = INSTALL.try_lock().map_err(|_| anyhow!("Modellen för exakta ordtider hämtas redan."))?;
    std::fs::create_dir_all(dir)?;
    for (file, expected) in native::FILES {
        work::check()?;
        let path = dir.join(file);
        if native::hash(&path).ok().as_deref() == Some(expected) {
            continue;
        }
        progress(&format!("Hämtar modell för exakta ordtider: {file}…"));
        let pending = dir.join(format!("{file}.download"));
        crate::download::to_file(&format!("{base}/{file}"), &pending, bytes)?;
        ensure!(native::hash(&pending)? == expected, "Kontrollsumman stämmer inte för {file}. Försök hämta modellen igen.");
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        std::fs::rename(pending, path)?;
    }
    progress("Exakta ordtider är klara att använda");
    Ok(())
}

fn ensure_loaded(cache: &mut Cache<Loaded>, dir: &Path) -> Result<()> {
    if cache.value.as_ref().is_some_and(|m| m.dir == dir) {
        return Ok(());
    }
    cache.clear();
    ensure!(ready(dir), "Modellen för exakta ordtider saknas. Öppna Modeller på datorn och välj Hämta modell.");
    let vocab = native::Vocab::from_json(&std::fs::read_to_string(dir.join(native::VOCAB_FILE))?)?;
    let model = dir.join(native::MODEL_FILE);
    let threads = num_cpus::get_physical().clamp(1, 8);
    work::check()?;
    let emitter = match native::Emitter::load(&model, native::Device::DirectMl, threads) {
        Ok(e) => e,
        Err(gpu) => {
            eprintln!("AVskrift ordjustering: DirectML otillgängligt ({gpu}); använder CPU");
            if !memory::sample().cpu_fits(CPU_BYTES) {
                crate::llm::release_cached();
            }
            ensure!(
                memory::sample().cpu_fits(CPU_BYTES),
                "För lite ledigt arbetsminne för exakta ordtider. Stäng andra program och försök igen."
            );
            native::Emitter::load(&model, native::Device::Cpu, threads)?
        }
    };
    work::check()?;
    cache.value = Some(Loaded { dir: dir.to_path_buf(), emitter, vocab });
    Ok(())
}

/// Replace Whisper's word times in `utterances` with acoustically aligned ones and tighten each
/// utterance to its words. Words that could not be aligned keep an interpolated time between their
/// neighbours. `samples` is 16 kHz mono, the same audio that was transcribed.
pub fn refine(
    dir: &Path,
    samples: &[f32],
    utterances: &mut [Utterance],
    progress: &dyn Fn(&str),
    pct: impl Fn(i32),
) -> Result<Refined> {
    let _work = memory::enter()?;
    progress("Laddar modell för exakta ordtider…");
    let mut cache = CACHE.lock().map_err(|_| anyhow!("Ordjusteringen behöver startas om."))?;
    ensure_loaded(&mut cache, dir)?;
    let loaded = cache.value.as_mut().unwrap();
    let device = match loaded.emitter.device {
        native::Device::DirectMl => "GPU (DirectML)",
        native::Device::Cpu => "CPU",
    };
    progress(&format!("Finjusterar ordtider – {device}…"));
    let em = match loaded.emitter.emissions(samples, |p| {
        pct(p);
        work::check().is_ok()
    }) {
        Ok(em) => em,
        Err(e) => {
            work::check()?; // a cancel is reported as such, not as a model failure
            cache.clear();
            return Err(e);
        }
    };
    let words: Vec<native::InputWord> = utterances
        .iter()
        .flat_map(|u| &u.words)
        .map(|w| native::InputWord { text: w.text.clone(), start: w.start, end: w.end })
        .collect();
    let aligned = native::align_words(&em, &loaded.vocab, &words);
    let mut it = aligned.iter();
    for u in utterances.iter_mut() {
        for w in u.words.iter_mut() {
            if let Some(a) = it.next() {
                (w.start, w.end) = (a.start, a.end);
            }
        }
        if let (Some(first), Some(last)) = (u.words.first(), u.words.last()) {
            (u.start, u.end) = (first.start, last.end.max(first.start));
        }
    }
    let sounds = native::unclaimed(&em, &loaded.vocab, &aligned);
    let loudness = native::Loudness::new(samples, native::RATE);
    let mut items: Vec<(f64, f64)> = aligned.iter().filter(|w| w.aligned).map(|w| (w.start, w.end)).collect();
    items.extend(sounds.iter().map(|s| (s.start, s.end)));
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let pauses = loudness.pauses(items.windows(2).map(|p| (p[0].1, p[1].0)));
    cache.touch();
    Ok(Refined { sounds, pauses, device })
}

pub fn release_cached() {
    if let Ok(mut c) = CACHE.try_lock() {
        c.clear();
    }
}
pub fn sweep(now: std::time::Instant, pressure: bool) {
    if let Ok(mut c) = CACHE.try_lock() {
        c.sweep(now, pressure);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Word;

    /// Opt-in network test: downloads and verifies the published model into
    /// AVSKRIFT_WORDALIGN_INSTALL_DIR (a scratch dir, never the user's appdata).
    #[test]
    #[ignore]
    fn install_from_source() {
        let dir = PathBuf::from(std::env::var("AVSKRIFT_WORDALIGN_INSTALL_DIR").unwrap());
        install(&dir, &|m| println!("{m}"), &|_, _| {}).unwrap();
        assert!(ready(&dir));
        for (file, expected) in native::FILES {
            assert_eq!(native::hash(&dir.join(file)).unwrap(), expected);
        }
        // A second install verifies the existing files and downloads nothing.
        let started = std::time::Instant::now();
        install(&dir, &|_| {}, &|_, _| panic!("must not download again")).unwrap();
        println!("INSTALL ok, re-check {:.1}s", started.elapsed().as_secs_f64());
    }

    /// Opt-in: AVSKRIFT_WORDALIGN_TEST_MODEL (dir with model.fp16.onnx + vocab.json) and
    /// AVSKRIFT_WORDALIGN_TEST_AUDIO (16 kHz mono WAV of someone saying the words in
    /// AVSKRIFT_WORDALIGN_TEST_TEXT). Never reads the user's appdata.
    #[test]
    #[ignore]
    fn refine_real_audio() {
        let dir = PathBuf::from(std::env::var("AVSKRIFT_WORDALIGN_TEST_MODEL").unwrap());
        let audio = crate::audio::load(Path::new(&std::env::var("AVSKRIFT_WORDALIGN_TEST_AUDIO").unwrap())).unwrap();
        let text = std::env::var("AVSKRIFT_WORDALIGN_TEST_TEXT").unwrap();
        let n = text.split_whitespace().count() as f64;
        let words: Vec<Word> = text
            .split_whitespace()
            .enumerate()
            .map(|(i, t)| {
                let s = i as f64 * audio.duration_s / n;
                Word { start: s, end: s + audio.duration_s / n, text: t.into() }
            })
            .collect();
        let mut utt = vec![Utterance { start: 0.0, end: audio.duration_s, speaker: None, text, words }];
        let r = refine(&dir, &audio.samples, &mut utt, &|m| println!("{m}"), |_| {}).unwrap();
        assert!(utt[0].words.windows(2).all(|w| w[0].start <= w[1].start));
        if let Ok(out) = std::env::var("AVSKRIFT_WORDALIGN_TEST_OUT") {
            let words: Vec<_> =
                utt[0].words.iter().map(|w| serde_json::json!({"text": w.text, "start": w.start, "end": w.end})).collect();
            std::fs::write(out, serde_json::to_string(&serde_json::json!({"words": words})).unwrap()).unwrap();
        }
        println!("REFINE device={} words={} sounds={} pauses={}", r.device, utt[0].words.len(), r.sounds.len(), r.pauses.len());
    }
}
