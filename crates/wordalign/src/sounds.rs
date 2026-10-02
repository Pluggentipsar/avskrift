//! Speech the transcript does not contain: fillers KB-Whisper cleaned away ("eh", "öh"),
//! restarts and untranscribed words. The CTC model still emits letters for them; letters outside
//! every aligned word become a `[ljud]` block that can be struck like a word.

use serde::Serialize;

use crate::{align::AlignedWord, model::Emissions, normalize::Vocab, FRAME_SECONDS};

/// Frames around an aligned word's label span that still belong to it (CTC spans are short peaks).
const CLAIM_PAD: usize = 2;
/// Letter frames closer than this are one block.
const JOIN_FRAMES: usize = 10;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SoundBlock {
    pub start: f64,
    pub end: f64,
    /// Greedy reading of the model, e.g. "ÖH"; a hint for the UI, not a transcript.
    pub heard: String,
    /// Mean probability of the letters read.
    pub score: f32,
}

pub fn unclaimed(em: &Emissions, vocab: &Vocab, words: &[AlignedWord]) -> Vec<SoundBlock> {
    let frames = em.frames();
    let mut claimed = vec![false; frames];
    for w in words.iter().filter(|w| w.aligned) {
        let s = ((w.start / FRAME_SECONDS).round() as usize).saturating_sub(CLAIM_PAD);
        let e = ((w.end / FRAME_SECONDS).round() as usize + CLAIM_PAD).min(frames);
        claimed[s.min(frames)..e].iter_mut().for_each(|c| *c = true);
    }
    let best = |t: usize| {
        let row = em.rows(t, t + 1);
        let (id, lp) = row.iter().enumerate().fold((0, f32::NEG_INFINITY), |a, (i, &v)| if v > a.1 { (i, v) } else { a });
        (id as u32, lp.exp())
    };
    let mut blocks = Vec::new();
    let mut current: Option<(usize, usize, Vec<(u32, f32)>)> = None; // first, last letter frame, letters
    let close = |b: (usize, usize, Vec<(u32, f32)>), out: &mut Vec<SoundBlock>| {
        let (first, last, letters) = b;
        let mut heard = String::new();
        let mut prev = None;
        for &(id, _) in &letters {
            if prev != Some(id) {
                heard.extend(vocab.char_of(id));
            }
            prev = Some(id);
        }
        // A single stray letter frame is noise, not a filler.
        if letters.len() >= 2 && !heard.is_empty() {
            let score = letters.iter().map(|l| l.1).sum::<f32>() / letters.len() as f32;
            let secs = |f: usize| (f as f64 * FRAME_SECONDS * 1000.0).round() / 1000.0;
            out.push(SoundBlock { start: secs(first), end: secs(last + 1), heard, score });
        }
    };
    for t in 0..frames {
        let (id, p) = best(t);
        let letter = id != vocab.blank && vocab.char_of(id).is_some();
        if claimed[t] {
            if let Some(b) = current.take() {
                close(b, &mut blocks);
            }
            continue;
        }
        if !letter {
            if current.as_ref().is_some_and(|b| t - b.1 > JOIN_FRAMES) {
                close(current.take().unwrap(), &mut blocks);
            }
            continue;
        }
        match current.as_mut() {
            Some(b) => {
                b.1 = t;
                b.2.push((id, p));
            }
            None => current = Some((t, t, vec![(id, p)])),
        }
    }
    if let Some(b) = current {
        close(b, &mut blocks);
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_outside_words_become_blocks() {
        let vocab = Vocab::from_json(r#"{"<pad>":0,"E":1,"H":2}"#).unwrap();
        // Frames: word "E" at 5, then "EH" (E E H) at 30-32 not in transcript, stray H at 70.
        let mut ids = vec![0u32; 80];
        ids[5] = 1;
        ids[30] = 1;
        ids[31] = 1;
        ids[32] = 2;
        ids[70] = 2;
        let lp = ids
            .iter()
            .flat_map(|&id| (0..3u32).map(move |v| if v == id { 0.9f32.ln() } else { 0.05f32.ln() }))
            .collect();
        let em = Emissions { lp, vocab: 3 };
        let words = [AlignedWord { text: "e".into(), start: 0.1, end: 0.12, score: 0.9, aligned: true }];
        let got = unclaimed(&em, &vocab, &words);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].start, got[0].end, got[0].heard.as_str()), (0.6, 0.66, "EH"));
    }
}
