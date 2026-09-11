//! Token budgets always measure the complete serialized prompt, using the inference tokenizer.
use anyhow::{bail, Result};

pub trait TextEngine {
    fn token_count(&self, text: &str) -> Result<usize>;
    fn context_limit(&self) -> usize;
    fn complete(&self, prompt: &str, output: usize) -> Result<String>;
}

pub fn fits(engine: &impl TextEngine, prompt: &str, output: usize) -> Result<bool> {
    Ok(engine.token_count(prompt)?.checked_add(output).is_some_and(|n| n <= engine.context_limit()))
}

pub fn validate_overhead(engine: &impl TextEngine, prompt: &str, output: usize) -> Result<()> {
    // Leave a little usable room for source text, rather than making thousands of tiny calls.
    if !fits(engine, prompt, output.saturating_add(64))? {
        bail!("Mallen eller frågan är för lång för modellens arbetsfönster. Förkorta instruktionerna och försök igen. Underlaget och tidigare resultat finns kvar.");
    }
    Ok(())
}

/// Lossless UTF-8 slices. Exponential probing bounds tokenizer work on very large inputs;
/// every emitted prefix is tested against the full prompt, not a character/token estimate.
pub fn split_text(text: &str, fits: impl Fn(&str) -> Result<bool>) -> Result<Vec<String>> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        let mut boundaries: Vec<usize> = remaining.char_indices().take(65_536).map(|(i, _)| i).collect();
        // A bounded probe prevents repeated tokenization/allocation of the entire remaining file.
        let end = remaining.char_indices().nth(boundaries.len()).map(|(i, _)| i).unwrap_or(remaining.len());
        boundaries.push(end);
        let count = boundaries.len() - 1;
        let mut good = 0;
        let mut probe = count.min(256);
        let bad;
        loop {
            if fits(&remaining[..boundaries[probe]])? {
                good = probe;
                if probe == count {
                    bad = count;
                    break;
                }
                probe = (probe * 2).min(count);
            } else {
                bad = probe;
                break;
            }
        }
        let mut high = bad;
        while good + 1 < high {
            let mid = (good + high) / 2;
            if fits(&remaining[..boundaries[mid]])? {
                good = mid;
            } else {
                high = mid;
            }
        }
        if good == 0 {
            bail!("Instruktionerna lämnar inte plats för underlaget. Förkorta mallen eller frågan.");
        }
        let mut end = boundaries[good];
        if end < remaining.len() {
            // Prefer a paragraph/line boundary, then whitespace, within the final quarter.
            let floor = end * 3 / 4;
            let prefix = &remaining[..end];
            let preferred = prefix
                .char_indices()
                .rev()
                .find(|(i, c)| *i >= floor && *c == '\n')
                .or_else(|| prefix.char_indices().rev().find(|(i, c)| *i >= floor && c.is_whitespace()));
            if let Some((i, c)) = preferred {
                let candidate = i + c.len_utf8();
                // BPE counts are not strictly monotonic at boundaries: recheck the chosen cut.
                if fits(&remaining[..candidate])? {
                    end = candidate;
                }
            }
        }
        result.push(remaining[..end].to_string());
        remaining = &remaining[end..];
    }
    Ok(result)
}

pub fn chunks(
    engine: &impl TextEngine,
    text: &str,
    output: usize,
    prompt: impl Fn(&str) -> String,
) -> Result<Vec<String>> {
    validate_overhead(engine, &prompt(""), output)?;
    split_text(text, |piece| fits(engine, &prompt(piece), output))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn splitting_preserves_unicode_newlines_and_unbroken_text_exactly() {
        for text in ["Åsa\r\nÖsten 👩🏽‍💻 漢字\n\n".repeat(80), "å👩漢".repeat(500), "a".repeat(140_000)]
        {
            let parts = split_text(&text, |s| Ok(s.len() <= 73)).unwrap();
            assert_eq!(parts.concat(), text);
            assert!(parts.iter().all(|s| !s.is_empty() && s.len() <= 73));
        }
        assert!(split_text("Å", |_| Ok(false)).is_err());
        assert!(split_text("Å", |_| anyhow::bail!("tokenizer failed")).is_err());
    }
    #[test]
    fn every_cut_is_rechecked_even_with_non_monotonic_counts() {
        let fits = |s: &str| Ok(s.len() <= 50 && s.len() != 49);
        let parts = split_text(&"x ".repeat(100), fits).unwrap();
        assert!(parts.iter().all(|s| fits(s).unwrap()));
        assert_eq!(parts.concat(), "x ".repeat(100));
    }
}
