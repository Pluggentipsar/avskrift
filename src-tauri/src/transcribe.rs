//! Speech-to-text via whisper.cpp (`whisper-rs`) using KB-Whisper GGML models.
//!
//! The context is loaded lazily and kept across calls; switching model id reloads it. Output is a
//! flat list of timed segments (~sentence/phrase level), each optionally carrying word-level
//! timestamps, which `align` then attributes to speakers.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

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
                return Err(anyhow!("Whisper-modellen '{id}' är inte nedladdad. Hämta den först (se modellväljaren)."));
            }
            // `mut` is only consumed by the GPU-feature `use_gpu` call below; without a GPU
            // backend feature the binding is never mutated, so silence the lint in that case.
            #[cfg_attr(not(any(feature = "cuda", feature = "metal", feature = "vulkan")), allow(unused_mut))]
            let mut cparams = WhisperContextParameters::default();
            // Use the GPU when the binary was built with a GPU backend feature.
            #[cfg(any(feature = "cuda", feature = "metal", feature = "vulkan"))]
            cparams.use_gpu(true);
            let ctx = WhisperContext::new_with_params(
                path.to_str().ok_or_else(|| anyhow!("ogiltig sökväg till modell"))?,
                cparams,
            )
            .with_context(|| format!("kunde inte ladda Whisper-modellen {}", path.display()))?;
            self.loaded = Some((id.to_string(), ctx));
        }
        Ok(&self.loaded.as_ref().unwrap().1)
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
        progress(&format!("Laddar modell ({id})…"));
        let ctx = self.ensure(id, path)?;
        let mut state = ctx.create_state().map_err(|e| anyhow!("kunde inte skapa Whisper-state: {e}"))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        if language != "auto" {
            params.set_language(Some(language));
        }
        params.set_translate(translate);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_token_timestamps(word_timestamps);
        // Live percent progress from whisper.cpp (0–100). Verify this API name against the pinned
        // whisper-rs version (see FINISH.md); older versions use `set_progress_callback`.
        params.set_progress_callback_safe(move |p: i32| pct(p));
        // whisper.cpp scales best up to *physical* cores; logical/SMT over-subscription (e.g. 15
        // threads on an 8-core CPU) just adds scheduling overhead. Use the physical core count.
        let threads = num_cpus::get_physical().max(1) as i32;
        params.set_n_threads(threads);

        progress("Transkriberar…");
        state.full(params, samples).map_err(|e| anyhow!("transkriberingen misslyckades: {e}"))?;

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
