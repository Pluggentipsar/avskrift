//! Built-in term lists compiled into the binary (no external files for the end user).
//! Provides the Diagnos, Medicin and Person (name) gazetteers; new sets can be added the same way.

use once_cell::sync::Lazy;

use super::dictionary::Dictionary;
use super::{Category, Span};

const DIAGNOSER: &str = include_str!("../data/diagnoser.txt");
const DIAGNOSER_AKRONYMER: &str = include_str!("../data/diagnoser_akronymer.txt");
const MEDICINER: &str = include_str!("../data/mediciner.txt");
const FORNAMN: &str = include_str!("../data/fornamn.txt");
const EFTERNAMN: &str = include_str!("../data/efternamn.txt");

/// Names double as everyday words (Björn, Sten, My), so a list match is only a hint, not a
/// certainty. We mark hits with a low score; the user reviews them. (Overlap resolution in
/// `merge` goes by source priority, not score, so this is a transparency signal — the real
/// safety net against over-masking is the manual review step.)
const NAME_SCORE: f32 = 0.5;

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
// Case-sensitive: require the capitalised form so the everyday-word sense of a name
// ("sten", "björn") is left alone while the proper-noun form ("Sten", "Björn") is caught.
static FORNAMN_WORDS: Lazy<Dictionary> =
    Lazy::new(|| Dictionary::new(&parse(FORNAMN), Category::Person, false));
static EFTERNAMN_WORDS: Lazy<Dictionary> =
    Lazy::new(|| Dictionary::new(&parse(EFTERNAMN), Category::Person, false));

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

/// Detect Swedish first and last names from the built-in name lists (SCB).
/// A deterministic safety net complementing the NER model: high recall on common names
/// (children's and staff names in school transcripts), at a low score because common
/// words double as names. Capitalisation is required to avoid the everyday-word sense.
pub fn namn(text: &str) -> Vec<Span> {
    let mut v = FORNAMN_WORDS.detect(text);
    v.extend(EFTERNAMN_WORDS.detect(text));
    for s in &mut v {
        s.score = NAME_SCORE;
    }
    v
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

    #[test]
    fn finds_first_and_last_names() {
        let hits = namn("Eleven Erik Andersson var med på mötet.");
        assert_eq!(hits.len(), 2);
        assert!(hits.iter().all(|s| s.category == Category::Person));
        assert!(hits.iter().all(|s| s.score < 1.0));
    }

    #[test]
    fn name_requires_capital_to_avoid_everyday_word() {
        // "Sten" the name is masked; "sten" the rock is left alone.
        assert_eq!(namn("Vi pratade med Sten igår.").len(), 1);
        assert!(namn("Han snubblade på en sten.").is_empty());
    }
}
