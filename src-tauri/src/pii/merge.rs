//! Combine spans from all detectors, resolve overlaps by priority, and apply replacements.

use std::collections::HashSet;

use serde::Serialize;

use super::pseudonym::Pseudonymizer;
use super::{Category, Source, Span};

fn priority(s: &Span) -> u8 {
    match s.source {
        Source::Manual => 5,     // explicit user click — always wins
        Source::Rule => 4,       // validated, structured PII
        Source::Dictionary => 3, // explicit user terms / built-in gazetteers
        Source::Model => 2,      // NER (carries a real category)
        Source::Ai => 1,         // LLM suggestion (yields to better-categorized hits)
    }
}

/// Keep only enabled categories and drop overlaps, preferring higher priority then longer spans.
/// Returns spans sorted by start offset.
pub fn resolve(spans: Vec<Span>, enabled: &HashSet<Category>) -> Vec<Span> {
    let mut cand: Vec<Span> = spans.into_iter().filter(|s| s.end > s.start && enabled.contains(&s.category)).collect();

    cand.sort_by(|a, b| {
        priority(b).cmp(&priority(a)).then((b.end - b.start).cmp(&(a.end - a.start))).then(a.start.cmp(&b.start))
    });

    let mut kept: Vec<Span> = Vec::new();
    for s in cand {
        let overlaps = kept.iter().any(|k| s.start < k.end && k.start < s.end);
        if !overlaps {
            kept.push(s);
        }
    }
    kept.sort_by_key(|s| s.start);
    kept
}

#[derive(Debug, Clone, Serialize)]
pub struct Replacement {
    pub start: usize,
    pub end: usize,
    pub original: String,
    pub replacement: String,
    pub category: Category,
}

pub struct AppliedResult {
    pub text: String,
    /// Structured record of every applied replacement. Populated by `apply`; not read yet
    /// (kept for a future "what was masked" view / audit log).
    #[allow(dead_code)]
    pub replacements: Vec<Replacement>,
}

/// Build the anonymized text by replacing each span (must be sorted, non-overlapping) with its
/// pseudonym. Numbering follows first appearance in the document.
pub fn apply(text: &str, spans: &[Span], pseudo: &mut Pseudonymizer) -> AppliedResult {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    let mut replacements = Vec::new();

    for span in spans {
        if span.start < cursor {
            continue; // defensive: skip any residual overlap
        }
        out.push_str(&text[cursor..span.start]);
        let label = span.custom.clone().unwrap_or_else(|| pseudo.label_for(span.category, &span.text));
        out.push_str(&label);
        replacements.push(Replacement {
            start: span.start,
            end: span.end,
            original: span.text.clone(),
            replacement: label,
            category: span.category,
        });
        cursor = span.end;
    }
    out.push_str(&text[cursor..]);

    AppliedResult { text: out, replacements }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_all() -> HashSet<Category> {
        Category::ALL.into_iter().collect()
    }

    #[test]
    fn higher_priority_wins_overlap() {
        // A model "person" span overlapping a rule personnummer span: rule wins.
        let spans = vec![
            Span::new(0, 11, "900101-1234", Category::Person, Source::Model, 0.8),
            Span::new(0, 11, "900101-1234", Category::Personnummer, Source::Rule, 1.0),
        ];
        let kept = resolve(spans, &enabled_all());
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].category, Category::Personnummer);
    }

    #[test]
    fn disabled_category_filtered() {
        let spans = vec![Span::new(0, 4, "Lund", Category::Plats, Source::Model, 0.9)];
        let enabled: HashSet<Category> = [Category::Person].into_iter().collect();
        assert!(resolve(spans, &enabled).is_empty());
    }

    #[test]
    fn applies_consistent_pseudonyms() {
        // Byte offsets ("ä" is 2 bytes, so "träffade" occupies bytes 5..14).
        let text = "Anna träffade Anna och Erik.";
        let spans = vec![
            Span::new(0, 4, "Anna", Category::Person, Source::Model, 0.9),
            Span::new(15, 19, "Anna", Category::Person, Source::Model, 0.9),
            Span::new(24, 28, "Erik", Category::Person, Source::Model, 0.9),
        ];
        let resolved = resolve(spans, &enabled_all());
        let mut p = Pseudonymizer::new();
        let res = apply(text, &resolved, &mut p);
        assert_eq!(res.text, "Person 1 träffade Person 1 och Person 2.");
    }
}
