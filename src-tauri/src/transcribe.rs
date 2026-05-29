//! Speech-to-text via whisper.cpp (`whisper-rs`) using KB-Whisper GGML models.
//!
//! The context is loaded lazily and kept across calls; switching model id reloads it. Output is a
//! flat list of timed segments at the whisper segment granularity (~sentence/phrase level), which
//! `align` then attributes to speakers.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// One recognised segment with absolute timestamps in seconds.
#[derive(Debug, Clone)]
pub struct RawSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

pub struct Transcriber {
    /// (model id, loaded context). Reloaded when the requested id changes.
    loaded: Option<(String, WhisperContext)>,
}

impl Transcriber {
    pub fn new() -> Self {
        Transcriber { loaded: None }
    }

    fn ensure(&mut self, id: &str, path: &Path) -> Result<&WhisperContext> {
        let needs_load = !matches!(&self.loaded, Some((cur, _)) if cur == id);
        if needs_load {
            if !path.exists() {
                return Err(anyhow!(
                    "Whisper-modellen '{id}' är inte nedladdad. Hämta den först (se modellväljaren)."
                ));
            }
            let ctx = WhisperContext::new_with_params(
                path.to_str().ok_or_else(|| anyhow!("ogiltig sökväg till modell"))?,
                WhisperContextParameters::default(),
            )
            .with_context(|| format!("kunde inte ladda Whisper-modellen {}", path.display()))?;
            self.loaded = Some((id.to_string(), ctx));
        }
        Ok(&self.loaded.as_ref().unwrap().1)
    }

    /// Transcribe 16 kHz mono f32 `samples`. `language` is an ISO code, or "auto" to detect.
    pub fn transcribe(
        &mut self,
        id: &str,
        path: &Path,
        samples: &[f32],
        language: &str,
        progress: &dyn Fn(&str),
    ) -> Result<Vec<RawSegment>> {
        progress(&format!("Laddar modell ({id})…"));
        let ctx = self.ensure(id, path)?;
        let mut state = ctx.create_state().map_err(|e| anyhow!("kunde inte skapa Whisper-state: {e}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        if language != "auto" {
            params.set_language(Some(language));
        }
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        // Use the available physical cores, leaving headroom for the UI.
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        params.set_n_threads(threads.saturating_sub(1).max(1) as i32);

        progress("Transkriberar…");
        state.full(params, samples).map_err(|e| anyhow!("transkriberingen misslyckades: {e}"))?;

        let n = state.full_n_segments().map_err(|e| anyhow!("kunde inte läsa segment: {e}"))?;
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let text = state
                .full_get_segment_text(i)
                .map_err(|e| anyhow!("kunde inte läsa segmenttext: {e}"))?;
            let t0 = state.full_get_segment_t0(i).map_err(|e| anyhow!("tidsfel: {e}"))?;
            let t1 = state.full_get_segment_t1(i).map_err(|e| anyhow!("tidsfel: {e}"))?;
            let text = text.trim().to_string();
            if text.is_empty() {
                continue;
            }
            // whisper timestamps are in centiseconds (10 ms units).
            out.push(RawSegment { start: t0 as f64 / 100.0, end: t1 as f64 / 100.0, text });
        }
        Ok(out)
    }
}
