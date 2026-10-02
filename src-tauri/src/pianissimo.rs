//! App model lifecycle. Lock order matches Whisper: WORK, then model cache.
use crate::{
    memory::{self, Cache},
    transcribe::RawSegment,
    work,
};
use anyhow::{anyhow, ensure, Result};
use avskrift_pianissimo as native;
use once_cell::sync::Lazy;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
struct Loaded {
    path: PathBuf,
    model: native::model::Model,
}
static CACHE: Lazy<Mutex<Cache<Loaded>>> = Lazy::new(|| Mutex::new(Cache::new()));
static INSTALL: Mutex<()> = Mutex::new(());

pub fn validate(language: &str, translate: bool) -> Result<()> {
    ensure!(
        matches!(language, "sv" | "auto") && !translate,
        "Pianissimo stöder bara svensk transkribering. Välj svenska utan översättning, eller välj KB-Whisper."
    );
    Ok(())
}
pub fn install(dir: &Path, progress: &dyn Fn(&str), bytes: &dyn Fn(u64, u64)) -> Result<()> {
    let _install = INSTALL.try_lock().map_err(|_| anyhow!("Pianissimo förbereds redan."))?;
    std::fs::create_dir_all(dir)?;
    for (file, expected) in native::FILES {
        work::check()?;
        let path = dir.join(file);
        if native::hash(&path).ok().as_deref() == Some(expected) {
            continue;
        }
        progress(&format!("Hämtar Pianissimo: {file}…"));
        let pending = dir.join(format!("{file}.download"));
        crate::download::to_file(
            &format!("https://huggingface.co/{}/resolve/{}/{file}", native::REPO, native::REVISION),
            &pending,
            bytes,
        )?;
        ensure!(
            native::hash(&pending)? == *expected,
            "Kontrollsumman stämmer inte för {file}. Försök hämta modellen igen."
        );
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        std::fs::rename(pending, path)?;
    }
    progress("Kontrollerar Pianissimo…");
    native::mark_verified(dir)?;
    progress("Pianissimo är klar att använda");
    Ok(())
}
fn ensure_loaded(cache: &mut Cache<Loaded>, dir: &Path) -> Result<()> {
    if cache.value.as_ref().is_some_and(|m| m.path == dir) {
        return Ok(());
    }
    cache.clear();
    crate::transcribe::release_cached();
    if memory::pageable_cpu_budget().may_page(2 * 1024 * memory::MIB) {
        crate::llm::release_cached();
    }
    admit_memory(2 * 1024 * memory::MIB, "köra")?;
    ensure!(
        native::ready(dir),
        "Pianissimo behöver hämtas. Öppna Modeller på datorn och välj Hämta modell."
    );
    native::verify(dir)?;
    work::check()?;
    let (model, _) = native::model::Model::load(dir, num_cpus::get_physical().clamp(1, 8), false, true)?;
    work::check()?;
    cache.value = Some(Loaded { path: dir.to_path_buf(), model });
    Ok(())
}
pub fn try_prepare(dir: &Path) -> Result<bool> {
    let Ok(_work) = memory::WORK.try_lock() else { return Ok(false) };
    let mut cache = CACHE.lock().map_err(|_| anyhow!("Pianissimo behöver startas om."))?;
    ensure_loaded(&mut cache, dir)?;
    cache.touch();
    Ok(true)
}
pub fn transcribe(dir: &Path, samples: &[f32], progress: &dyn Fn(&str), pct: impl Fn(i32)) -> Result<Vec<RawSegment>> {
    let _work = memory::enter()?;
    let note = if memory::pageable_cpu_budget().may_page(2 * 1024 * memory::MIB) {
        " Lite ledigt RAM; bearbetningen kan ta längre tid."
    } else {
        ""
    };
    progress(&format!("Laddar Pianissimo – CPU…{note}"));
    let mut cache = CACHE.lock().map_err(|_| anyhow!("Pianissimo behöver startas om."))?;
    ensure_loaded(&mut cache, dir)?;
    progress(&format!("Transkriberar med Pianissimo – CPU…{note}"));
    let result = native::segments::transcribe(&mut cache.value.as_mut().unwrap().model, samples, work::check, pct);
    if result.is_err() {
        cache.clear();
    }
    cache.touch();
    Ok(result?.into_iter().map(|s| RawSegment { start: s.start, end: s.end, text: s.text, words: vec![] }).collect())
}
fn admit_memory(bytes: u64, action: &str) -> Result<bool> {
    let budget = memory::pageable_cpu_budget();
    let gib = |n: Option<u64>| {
        n.map(|b| format!("{:.1} GB", b as f64 / (1024.0 * memory::MIB as f64))).unwrap_or_else(|| "okänt".into())
    };
    ensure!(budget.fits(bytes),
        "För lite tillgängligt minne för att {action} Pianissimo. Ledigt RAM: {}. Ledigt virtuellt minne (RAM och växlingsfil): {}. Behöver {} tillgängligt minne och minst 256 MB ledigt RAM. Stäng andra program och försök igen.",
        gib(budget.ram_free), gib(budget.commit_free), gib(Some(bytes + 512 * memory::MIB)));
    Ok(budget.may_page(bytes))
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
pub fn cache_status() -> Option<String> {
    CACHE.try_lock().ok()?.value.as_ref().map(|_| "Pianissimo – CPU".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_options() {
        assert!(validate("sv", false).is_ok());
        assert!(validate("auto", false).is_ok());
        assert!(validate("en", false).is_err());
        assert!(validate("sv", true).is_err());
    }
    /// Run against a dedicated local test model directory, never the user's appdata.
    #[test]
    #[ignore]
    fn install_and_load() {
        let dir = PathBuf::from(std::env::var("AVSKRIFT_PIANISSIMO_TEST_MODEL").unwrap());
        let pool = ort::environment::GlobalThreadPoolOptions::default()
            .with_intra_threads(8)
            .unwrap()
            .with_inter_threads(1)
            .unwrap()
            .with_spin_control(true)
            .unwrap();
        assert!(ort::init().with_global_thread_pool(pool).commit());
        install(&dir, &|message| println!("{message}"), &|_, _| {
            panic!("Use verified local fixtures; no downloads in this test")
        })
        .unwrap();
        assert!(native::ready(&dir));
        let silence = vec![0.0; 5 * 16000];
        let result = transcribe(&dir, &silence, &|message| println!("{message}"), |_| {}).unwrap();
        assert!(result.is_empty());
        release_cached();
    }
    /// Explicit local fixtures only. Exercises the actual shared app path and export/diarisation.
    #[test]
    #[ignore]
    fn app_pipeline() {
        let dir = PathBuf::from(std::env::var("AVSKRIFT_PIANISSIMO_TEST_MODEL").unwrap());
        let wav = PathBuf::from(std::env::var("AVSKRIFT_PIANISSIMO_TEST_AUDIO").unwrap());
        let pool = ort::environment::GlobalThreadPoolOptions::default()
            .with_intra_threads(8)
            .unwrap()
            .with_inter_threads(1)
            .unwrap()
            .with_spin_control(true)
            .unwrap();
        assert!(ort::init().with_global_thread_pool(pool).commit());
        let audio = crate::audio::load(&wav).unwrap();
        let mut tr = crate::transcribe::Transcriber::new();
        let first = tr.transcribe(native::ID, &dir, &audio.samples, "sv", true, false, &|_| {}, |_| {}).unwrap();
        assert!(!first.is_empty());
        assert!(first.iter().all(|s| s.words.is_empty()));
        assert!(first.iter().all(|s| s.start >= 0.0 && s.start <= s.end && s.end <= audio.duration_s));
        assert!(tr.transcribe(native::ID, &dir, &audio.samples, "en", false, false, &|_| {}, |_| {}).is_err());
        let token = work::begin().unwrap();
        assert!(work::cancel(&token));
        let result = work::run(Some(token), || {
            tr.transcribe(native::ID, &dir, &audio.samples, "sv", false, false, &|_| {}, |_| {})
        });
        assert!(result.unwrap_err().is::<work::Cancelled>());
        let again = tr.transcribe(native::ID, &dir, &audio.samples, "sv", false, false, &|_| {}, |_| {}).unwrap();
        assert_eq!(
            first.iter().map(|s| &s.text).collect::<Vec<_>>(),
            again.iter().map(|s| &s.text).collect::<Vec<_>>()
        );
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/diarization");
        let turns = crate::diarize::diarize(
            &resources.join("segmentation.onnx"),
            &resources.join("embedding.onnx"),
            &audio.samples,
            Some(1),
            &|_| {},
        )
        .unwrap();
        let utterances = crate::align::with_speakers(again, &turns);
        assert!(utterances.iter().any(|u| u.speaker.is_some()));
        println!("Pianissimo app path: audio decoding, inference, option validation, cancellation/reuse, speaker alignment passed ({} segments)",utterances.len());
        let transcript = crate::transcript::Transcript {
            utterances,
            language: "sv".into(),
            model: native::ID.into(),
            diarized: true,
        };
        let labels = std::collections::BTreeMap::new();
        assert!(transcript.to_vtt(None, &labels).starts_with("WEBVTT"));
        assert!(transcript.to_srt(None, &labels).contains(" --> "));
        release_cached();
    }
}
