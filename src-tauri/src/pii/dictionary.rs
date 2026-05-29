//! Whole-word matching of a term list against text, tagged with a chosen category.
//! Used both for the user's custom dictionary (Egen) and for built-in gazetteers (e.g. Diagnos).

use aho_corasick::{AhoCorasick, MatchKind};

use super::{char_after_is_alnum, char_before_is_alnum, Category, Source, Span};

pub struct Dictionary {
    ac: Option<AhoCorasick>,
    category: Category,
}

impl Dictionary {
    pub fn new(terms: &[String], category: Category, case_insensitive: bool) -> Self {
        let clean: Vec<&str> = terms.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
        let ac = if clean.is_empty() {
            None
        } else {
            AhoCorasick::builder()
                .ascii_case_insensitive(case_insensitive)
                .match_kind(MatchKind::LeftmostLongest)
                .build(&clean)
                .ok()
        };
        Dictionary { ac, category }
    }

    pub fn detect(&self, text: &str) -> Vec<Span> {
        let Some(ac) = &self.ac else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for m in ac.find_iter(text) {
            let (s, e) = (m.start(), m.end());
            if char_before_is_alnum(text, s) || char_after_is_alnum(text, e) {
                continue;
            }
            out.push(Span::new(s, e, &text[s..e], self.category, Source::Dictionary, 1.0));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_whole_words_case_insensitive() {
        let d = Dictionary::new(
            &["Projekt Solros".to_string(), "Avdelning 5".to_string()],
            Category::Egen,
            true,
        );
        let hits = d.detect("Vi startade projekt solros på Avdelning 5.");
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn ignores_substring_inside_word() {
        let d = Dictionary::new(&["sol".to_string()], Category::Egen, true);
        assert!(d.detect("solros").is_empty());
    }

    #[test]
    fn case_sensitive_only_matches_exact() {
        let d = Dictionary::new(&["ADD".to_string()], Category::Diagnos, false);
        assert_eq!(d.detect("Diagnosen ADD ställdes").len(), 1);
        assert!(d.detect("vi vill add en rad").is_empty());
    }
}
