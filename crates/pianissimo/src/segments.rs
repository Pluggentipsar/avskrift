//! Sentence-sized playback intervals. Token starts are model estimates; word ends
//! are not supplied by this model, so we deliberately do not expose word timings.
use crate::model::{clean_text, Model, Transcript};
use anyhow::{ensure, Result};
#[derive(Debug, Clone, serde::Serialize)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}
const RATE: usize = 16000;
// The pinned export loses speech on long single passes. Keep each inference
// below the empirically verified 36-second range, including context.
const CORE: usize = 28 * RATE;
const CONTEXT: usize = 2 * RATE;

pub fn transcribe(
    model: &mut Model,
    samples: &[f32],
    check: impl Fn() -> Result<()>,
    progress: impl Fn(i32),
) -> Result<Vec<Segment>> {
    ensure!(
        samples.iter().all(|x| x.is_finite()),
        "Ogiltigt ljud: innehåller icke ändliga värden"
    );
    let mut result = Vec::new();
    let core = if samples.len() <= 36 * RATE {
        samples.len().max(1)
    } else {
        CORE
    };
    for begin in (0..samples.len()).step_by(core) {
        check()?;
        let end = (begin + core).min(samples.len());
        let left = begin.saturating_sub(CONTEXT);
        let right = (end + CONTEXT).min(samples.len());
        let mut audio = samples[left..right].to_vec();
        audio.resize(audio.len().max(512), 0.0);
        let (text, _) = model.transcribe(&audio, &check)?;
        result.extend(segments(
            &text,
            left as f64 / RATE as f64,
            begin as f64 / RATE as f64,
            end as f64 / RATE as f64,
        ));
        progress((end as f64 / samples.len() as f64 * 100.0) as i32);
    }
    check()?;
    progress(100);
    Ok(result)
}

pub fn segments(t: &Transcript, offset: f64, keep_start: f64, keep_end: f64) -> Vec<Segment> {
    let mut words: Vec<(f64, f64, String)> = Vec::new();
    for (piece, stamp) in t.tokens.iter().zip(&t.timestamps) {
        let time = offset + stamp;
        if piece.starts_with(char::is_whitespace) || words.is_empty() {
            words.push((time, time, piece.clone()));
        } else if let Some(w) = words.last_mut() {
            w.1 = time;
            w.2.push_str(piece);
        }
    }
    let mut result = Vec::new();
    let mut current: Option<Segment> = None;
    for (i, (start, last, text)) in words.iter().enumerate() {
        // The word's first token owns it. Context may contain repeated complete words,
        // but adjacent chunks have disjoint ownership intervals.
        if *start < keep_start || *start >= keep_end {
            continue;
        }
        let end = words
            .get(i + 1)
            .map(|w| w.0)
            .unwrap_or(last + 0.24)
            .min(last + 0.5)
            .max(*start)
            .min(keep_end);
        if current
            .as_ref()
            .is_some_and(|s| start - s.start >= 5.0 || start - s.end > 0.8)
        {
            result.push(current.take().unwrap());
        }
        let s = current.get_or_insert_with(|| Segment {
            start: *start,
            end,
            text: String::new(),
        });
        s.text.push_str(text);
        s.end = end;
        if text.trim_end().ends_with(['.', '!', '?']) {
            result.push(current.take().unwrap());
        }
    }
    if let Some(s) = current {
        result.push(s);
    }
    for s in &mut result {
        s.text = clean_text(&s.text).trim().to_owned();
    }
    result.retain(|s| !s.text.is_empty());
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn transcript(tokens: &[&str], stamps: &[f64]) -> Transcript {
        Transcript {
            text: String::new(),
            tokens: tokens.iter().map(|s| s.to_string()).collect(),
            timestamps: stamps.to_vec(),
            token_ids: vec![],
        }
    }
    #[test]
    fn punctuation_subwords_and_silence() {
        let t = transcript(
            &[" Å", "sa", " är", " här", ".", " Nej", "!"],
            &[0.0, 0.08, 0.4, 0.8, 0.9, 4.0, 4.2],
        );
        let s = segments(&t, 0.0, 0.0, 10.0);
        assert_eq!(
            s.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(),
            ["Åsa är här.", "Nej!"]
        );
        assert!(s[0].end < 2.0);
        assert_eq!(s[1].start, 4.0);
        assert!(segments(&transcript(&[], &[]), 0.0, 0.0, 5.0).is_empty());
    }
    #[test]
    fn overlap_assigns_whole_word_to_one_chunk() {
        let t = transcript(
            &[" före", " grän", "sen", " efter"],
            &[59.0, 59.9, 60.1, 60.4],
        );
        let a = segments(&t, 0.0, 0.0, 60.0);
        let b = segments(&t, 0.0, 60.0, 120.0);
        assert_eq!(
            a.iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            "före gränsen"
        );
        assert_eq!(b[0].text, "efter");
        assert!(a.iter().all(|s| s.start <= s.end && s.end <= 60.0));
    }
}
