//! Subtitles and text for the edited film: kept words mapped onto the edited timeline.

use crate::edl::to_edited;

const MAX_CHARS: usize = 84; // two lines of ~42
const LINE: usize = 42;
const MAX_SECONDS: f64 = 6.0;
const GAP: f64 = 1.0; // a pause this long (edited time) starts a new cue

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// A word's edited (start, end), when the whole word was kept. An end exactly on a cut belongs
/// to the range before it.
fn kept_word(keep: &[(f64, f64)], a: f64, b: f64) -> Option<(f64, f64)> {
    let start = to_edited(keep, a)?;
    let end = to_edited(keep, b).or_else(|| to_edited(keep, (b - 0.001).max(a)))?;
    (end >= start).then_some((start, end))
}

/// Words are `(text, source start, source end)` in order; only words fully inside kept ranges count.
pub fn cues(words: &[(String, f64, f64)], keep: &[(f64, f64)]) -> Vec<Cue> {
    let mapped: Vec<(String, f64, f64)> =
        words.iter().filter_map(|(w, a, b)| kept_word(keep, *a, *b).map(|(s, e)| (w.clone(), s, e))).collect();
    let mut out: Vec<Cue> = Vec::new();
    let mut cur: Option<Cue> = None;
    for (w, a, b) in mapped {
        if let Some(c) = cur.as_mut() {
            let ends_sentence = c.text.ends_with(['.', '!', '?']);
            if ends_sentence || a - c.end > GAP || b - c.start > MAX_SECONDS || c.text.len() + 1 + w.len() > MAX_CHARS {
                out.push(cur.take().unwrap());
            }
        }
        match cur.as_mut() {
            Some(c) => {
                c.text.push(' ');
                c.text.push_str(&w);
                c.end = b;
            }
            None => cur = Some(Cue { start: a, end: b, text: w }),
        }
    }
    out.extend(cur);
    // Keep each cue on screen at least a moment, without overlapping the next.
    for i in 0..out.len() {
        let next = out.get(i + 1).map_or(f64::INFINITY, |n| n.start);
        out[i].end = out[i].end.max(out[i].start + 0.8).min(next - 0.04).max(out[i].start + 0.04);
    }
    out
}

/// Break a cue into at most two lines at a space near the middle.
fn wrap(text: &str) -> String {
    if text.chars().count() <= LINE {
        return text.to_string();
    }
    let mid = text.len() / 2;
    let split = text
        .match_indices(' ')
        .map(|(i, _)| i)
        .min_by_key(|i| (*i as i64 - mid as i64).abs())
        .unwrap_or(mid);
    format!("{}\n{}", &text[..split], text[split..].trim_start())
}

fn stamp(t: f64, sep: char) -> String {
    let ms = (t.max(0.0) * 1000.0).round() as u64;
    format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn srt(cues: &[Cue]) -> String {
    cues.iter()
        .enumerate()
        .map(|(i, c)| format!("{}\n{} --> {}\n{}\n\n", i + 1, stamp(c.start, ','), stamp(c.end, ','), wrap(&c.text)))
        .collect()
}

pub fn vtt(cues: &[Cue]) -> String {
    let body: String =
        cues.iter().map(|c| format!("{} --> {}\n{}\n\n", stamp(c.start, '.'), stamp(c.end, '.'), wrap(&c.text))).collect();
    format!("WEBVTT\n\n{body}")
}

/// Plain text of the kept words, one paragraph per group (e.g. utterance).
pub fn text(paragraphs: &[Vec<(String, f64, f64)>], keep: &[(f64, f64)]) -> String {
    paragraphs
        .iter()
        .map(|p| {
            p.iter().filter(|(_, a, b)| kept_word(keep, *a, *b).is_some())
                .map(|w| w.0.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(t: &str, a: f64, b: f64) -> (String, f64, f64) {
        (t.into(), a, b)
    }

    #[test]
    fn cut_words_vanish_and_times_shift() {
        let words = [w("Hej", 0.0, 0.4), w("eh", 1.0, 1.3), w("där.", 2.0, 2.4), w("Nästa", 3.0, 3.5)];
        let keep = [(0.0, 0.5), (1.9, 4.0)]; // "eh" cut; 1.4 s removed
        let c = cues(&words, &keep);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].text, "Hej där.");
        assert!((c[0].start - 0.0).abs() < 1e-9 && (c[1].start - 1.6).abs() < 1e-9, "{c:?}");
        assert!(srt(&c).starts_with("1\n00:00:00,000 --> 00:00:01,"));
        assert!(vtt(&c).starts_with("WEBVTT\n\n00:00:00.000 --> "));
    }

    #[test]
    fn long_cues_split_and_wrap() {
        let words: Vec<_> = (0..30).map(|i| w("ordet", i as f64 * 0.3, i as f64 * 0.3 + 0.25)).collect();
        let c = cues(&words, &[(0.0, 20.0)]);
        assert!(c.len() >= 2 && c.iter().all(|c| c.text.len() <= 84 && c.end - c.start <= 6.0 + 1e-9));
        assert!(wrap(&c[0].text).contains('\n'));
    }

    #[test]
    fn text_keeps_paragraphs_of_kept_words() {
        let paras = vec![vec![w("A", 0.0, 0.5), w("b", 1.0, 1.5)], vec![w("C", 3.0, 3.5)]];
        assert_eq!(text(&paras, &[(0.0, 0.8), (2.9, 4.0)]), "A\n\nC\n");
    }
}
