//! CTC forced alignment (Viterbi over the blank-interleaved label sequence).

/// For each label, the first and last frame it occupies plus its mean probability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelSpan {
    pub first: usize,
    pub last: usize,
    pub prob: f32,
}

/// Align `labels` to log-probabilities `lp` (`frames × vocab`, row-major). Returns `None` when the
/// window has fewer frames than the labels need (each label one frame, repeats need a blank).
pub fn align(lp: &[f32], vocab: usize, labels: &[u32], blank: u32) -> Option<Vec<LabelSpan>> {
    let frames = lp.len() / vocab;
    if labels.is_empty() {
        return Some(Vec::new());
    }
    let repeats = labels.windows(2).filter(|w| w[0] == w[1]).count();
    if frames < labels.len() + repeats {
        return None;
    }
    let states = 2 * labels.len() + 1;
    let label = |s: usize| if s % 2 == 0 { blank } else { labels[s / 2] };
    // May jump s-2 -> s: s is a label that differs from the previous label.
    let skip: Vec<bool> = (0..states).map(|s| s % 2 == 1 && s >= 3 && labels[s / 2] != labels[s / 2 - 1]).collect();
    const NEG: f64 = f64::NEG_INFINITY;
    let mut score = vec![NEG; states];
    let mut next = vec![NEG; states];
    score[0] = lp[blank as usize] as f64;
    score[1] = lp[label(1) as usize] as f64;
    // 0 stay, 1 from s-1, 2 from s-2. One byte per cell: 60 min × ~20k labels is never aligned
    // in one window (see align.rs), so this stays small.
    let mut back = vec![0u8; frames * states];
    for t in 1..frames {
        let row = &lp[t * vocab..(t + 1) * vocab];
        for s in 0..states {
            let (mut best, mut from) = (score[s], 0u8);
            if s >= 1 && score[s - 1] > best {
                (best, from) = (score[s - 1], 1);
            }
            if skip[s] && score[s - 2] > best {
                (best, from) = (score[s - 2], 2);
            }
            next[s] = best + row[label(s) as usize] as f64;
            back[t * states + s] = from;
        }
        std::mem::swap(&mut score, &mut next);
    }
    let mut s = if score[states - 1] >= score[states - 2] { states - 1 } else { states - 2 };
    if score[s] == NEG {
        return None;
    }
    let mut spans: Vec<Option<(usize, usize, f64, usize)>> = vec![None; labels.len()];
    for t in (0..frames).rev() {
        if s % 2 == 1 {
            let p = (lp[t * vocab + label(s) as usize] as f64).exp();
            let e = spans[s / 2].get_or_insert((t, t, 0.0, 0));
            e.0 = t;
            e.2 += p;
            e.3 += 1;
        }
        s -= back[t * states + s] as usize;
    }
    spans.into_iter().map(|e| e.map(|(first, last, sum, n)| LabelSpan { first, last, prob: (sum / n as f64) as f32 })).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Log-probs where frame t strongly predicts `ids[t]`.
    fn peaks(ids: &[u32], vocab: usize) -> Vec<f32> {
        ids.iter()
            .flat_map(|&id| (0..vocab as u32).map(move |v| if v == id { (0.9f32).ln() } else { (0.1f32 / 3.0).ln() }))
            .collect()
    }

    #[test]
    fn labels_land_on_their_peaks() {
        // blank=0; labels 1,2 at frames 2-3 and 5.
        let lp = peaks(&[0, 0, 1, 1, 0, 2, 0], 4);
        let spans = align(&lp, 4, &[1, 2], 0).unwrap();
        assert_eq!((spans[0].first, spans[0].last), (2, 3));
        assert_eq!((spans[1].first, spans[1].last), (5, 5));
        assert!(spans[0].prob > 0.8);
    }

    #[test]
    fn repeated_labels_need_a_blank_between() {
        let lp = peaks(&[0, 3, 0, 3, 0], 4);
        let spans = align(&lp, 4, &[3, 3], 0).unwrap();
        assert_eq!((spans[0].first, spans[1].first), (1, 3));
        assert!(align(&peaks(&[3, 3], 4), 4, &[3, 3], 0).is_none());
    }
}
