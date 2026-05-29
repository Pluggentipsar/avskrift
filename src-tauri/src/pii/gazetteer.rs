//! Built-in term lists compiled into the binary (no external files for the end user).
//! Currently provides the Diagnos gazetteer; new sets can be added the same way.

use once_cell::sync::Lazy;

use super::dictionary::Dictionary;
use super::{Category, Span};

const DIAGNOSER: &str = include_str!("../data/diagnoser.txt");
const DIAGNOSER_AKRONYMER: &str = include_str!("../data/diagnoser_akronymer.txt");
const MEDICINER: &str = include_str!("../data/mediciner.txt");

fn parse(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

static DIAGNOS_WORDS: Lazy<Dictionary> =
    Lazy::new(|| Dictionary::new(&parse(DIAGNOSER), Category::Diagnos, true));
static DIAGNOS_ACRONYMS: Lazy<Dictionary> =
    Lazy::new(|| Dictionary::new(&parse(DIAGNOSER_AKRONYMER), Category::Diagnos, false));
static MEDICIN_WORDS: Lazy<Dictionary> =
    Lazy::new(|| Dictionary::new(&parse(MEDICINER), Category::Medicin, true));

/// Detect diagnosis terms from the built-in gazetteer (ICD codes are handled in `rules`).
pub fn diagnoser(text: &str) -> Vec<Span> {
    let mut v = DIAGNOS_WORDS.detect(text);
    v.extend(DIAGNOS_ACRONYMS.detect(text));
    v
}

/// Detect medication names from the built-in gazetteer.
pub fn mediciner(text: &str) -> Vec<Span> {
    MEDICIN_WORDS.detect(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_diagnoses_any_case() {
        let hits = diagnoser("Eleven har adhd och en autismspektrumtillstånd-utredning.");
        assert!(hits.iter().any(|s| s.category == Category::Diagnos));
        assert!(hits.len() >= 2);
    }

    #[test]
    fn acronym_requires_uppercase() {
        assert_eq!(diagnoser("Diagnosen ADD noterades").len(), 1);
        assert!(diagnoser("vi vill add detta").is_empty());
    }
}
