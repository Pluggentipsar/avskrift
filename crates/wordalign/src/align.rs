//! Window-wise forced alignment of a whole transcript.
//!
//! One Viterbi over an hour (180 000 frames × ~20 000 labels) would be slow and lets one
//! untranscribed passage pull every later word. Instead words are grouped into ~30 s runs by their
//! (rough) Whisper times and each run is aligned in a window from where the previous run ended to
//! a margin after its own estimated end. Runs therefore stay in order, and Whisper errors of a few
//! seconds — seen in the prototype — are still inside the window.

use serde::{Deserialize, Serialize};

use crate::{model::Emissions, normalize::Vocab, viterbi, FRAME_SECONDS};

/// Target length of one alignment run, by Whisper time.
const RUN_SECONDS: f64 = 30.0;
/// Search margin around a run's Whisper estimate.
const MARGIN_SECONDS: f64 = 5.0;
/// At most this many following words are aligned as context for a run's end.
const CONTEXT_WORDS: usize = 40;

#[derive(Debug, Clone, Deserialize)]
pub struct InputWord {
    pub text: String,
    /// Rough estimate (e.g. Whisper token time); only used to place search windows.
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AlignedWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
    /// Mean model probability of the word's labels; 0 when the word could not be aligned.
    pub score: f32,
    /// False when the word has no pronounceable labels or its run failed; times are then
    /// interpolated between neighbours and must not be trusted for cutting.
    pub aligned: bool,
}

pub fn align_words(em: &Emissions, vocab: &Vocab, words: &[InputWord]) -> Vec<AlignedWord> {
    let frames = em.frames();
    let to_frame = |s: f64| ((s.max(0.0) / FRAME_SECONDS) as usize).min(frames);
    let mut out: Vec<Option<(usize, usize, f32)>> = vec![None; words.len()];
    let mut floor = 0usize; // first frame the next run may use
    let mut i = 0;
    while i < words.len() {
        // Collect a run of ~RUN_SECONDS by estimate.
        let run_start = i;
        let t0 = words[i].start;
        while i < words.len() && (i == run_start || words[i].end - t0 < RUN_SECONDS) {
            i += 1;
        }
        // Following words are aligned as context but not kept: without them the run's last words
        // drift into the next run's speech, which is inside the window's margin.
        let run_end = words[i - 1].end;
        let mut ctx_end = i;
        while ctx_end < words.len() && words[ctx_end].start < run_end + MARGIN_SECONDS && ctx_end - i < CONTEXT_WORDS {
            ctx_end += 1;
        }
        let run = &words[run_start..ctx_end];
        let own = i - run_start;
        let mut labels = Vec::new();
        let mut owner = Vec::new();
        for (k, w) in run.iter().enumerate() {
            let ids = vocab.encode(&w.text);
            owner.extend(std::iter::repeat_n(k, ids.len()));
            labels.extend(ids);
        }
        if labels.is_empty() {
            continue;
        }
        let lo = floor.max(to_frame(t0 - MARGIN_SECONDS)).min(frames);
        let mut hi = to_frame(run.last().map_or(t0, |w| w.end) + MARGIN_SECONDS).max(lo);
        // Too few frames for the labels (fast speech, bad estimate): widen to the end of the file.
        if hi - lo < 2 * labels.len() {
            hi = frames;
        }
        let Some(spans) = viterbi::align(em.rows(lo, hi), em.vocab, &labels, vocab.blank) else {
            continue;
        };
        let mut words_spans: Vec<Option<(usize, usize, f32, usize)>> = vec![None; run.len()];
        for (span, &k) in spans.iter().zip(&owner) {
            let e = words_spans[k].get_or_insert((span.first, span.last, 0.0, 0));
            e.0 = e.0.min(span.first);
            e.1 = e.1.max(span.last);
            e.2 += span.prob;
            e.3 += 1;
        }
        for (k, ws) in words_spans.into_iter().enumerate().take(own) {
            if let Some((first, last, sum, n)) = ws {
                out[run_start + k] = Some((lo + first, lo + last + 1, sum / n as f32));
                floor = floor.max(lo + last + 1);
            }
        }
    }
    finish(words, &out)
}

/// Convert frame spans to seconds; interpolate words without a span between their neighbours.
fn finish(words: &[InputWord], spans: &[Option<(usize, usize, f32)>]) -> Vec<AlignedWord> {
    let secs = |f: usize| (f as f64 * FRAME_SECONDS * 1000.0).round() / 1000.0;
    (0..words.len())
        .map(|i| match spans[i] {
            Some((s, e, score)) => {
                AlignedWord { text: words[i].text.clone(), start: secs(s), end: secs(e), score, aligned: true }
            }
            None => {
                let prev = spans[..i].iter().rev().flatten().next().map(|p| secs(p.1));
                let next = spans[i + 1..].iter().flatten().next().map(|n| secs(n.0));
                let start = prev.unwrap_or(words[i].start);
                let end = next.unwrap_or(words[i].end).max(start);
                AlignedWord { text: words[i].text.clone(), start, end: start.max(end), score: 0.0, aligned: false }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab() -> Vocab {
        Vocab::from_json(r#"{"<pad>":0,"A":1,"B":2}"#).unwrap()
    }

    /// One frame per entry; 0 = blank, else strong peak on that label.
    fn emissions(ids: &[u32]) -> Emissions {
        let lp = ids
            .iter()
            .flat_map(|&id| (0..3u32).map(move |v| if v == id { 0.9f32.ln() } else { 0.05f32.ln() }))
            .collect();
        Emissions { lp, vocab: 3 }
    }

    #[test]
    fn words_get_acoustic_times_not_estimates() {
        // "a" at frames 10-11, "b" at frame 50 (0.2 s, 1.0 s); estimates are off by ~0.5 s.
        let mut ids = vec![0; 80];
        ids[10] = 1;
        ids[11] = 1;
        ids[50] = 2;
        let words = [
            InputWord { text: "a".into(), start: 0.7, end: 0.8 },
            InputWord { text: "–".into(), start: 0.8, end: 0.9 },
            InputWord { text: "b.".into(), start: 1.5, end: 1.6 },
        ];
        let got = align_words(&emissions(&ids), &vocab(), &words);
        assert_eq!((got[0].start, got[0].end, got[0].aligned), (0.2, 0.24, true));
        assert_eq!((got[2].start, got[2].end), (1.0, 1.02));
        // Unpronounceable word sits between its neighbours and is marked untrusted.
        assert_eq!((got[1].start, got[1].end, got[1].aligned), (0.24, 1.0, false));
    }
}
