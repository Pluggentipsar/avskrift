//! Optional semantic de-identification layer: a small local LLM (Qwen GGUF via llama.cpp, see
//! [`crate::llm`]) that proposes additional verbatim substrings to mask. Its output is treated as
//! just another candidate source that flows through the normal human review — never auto-trusted.

use std::path::Path;

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::llm::Qwen;

/// Matches a JSON string literal (handling escapes) — used to recover items from truncated output.
static JSON_STRING: Lazy<Regex> = Lazy::new(|| Regex::new(r#""((?:[^"\\]|\\.)*)""#).unwrap());

const SYSTEM_PROMPT: &str = "Du är ett verktyg för avidentifiering av svensk text. Lista ALLT som \
direkt eller indirekt kan identifiera en person eller röja känsliga personuppgifter (GDPR art. 9): \
namn, adresser, lägenhetsnummer, telefonnummer, e-post, personnummer; \
hälsa (diagnoser, läkemedel, doser, intoleranser, operationer); religion, etnicitet, sexuell \
läggning, fackligt/politiskt; samt indirekta ledtrådar (t.ex. \"det skyddade boendet\", \
\"skilsmässan\", en unik kombination av detaljer). Svara ENBART med en giltig JSON-array av exakta, \
ordagranna textutdrag ur texten. Ingen annan text.";

const EXAMPLE_IN: &str = "Anna Lind bor på Storgatan 3 lgh 12 och har Concerta mot sin ADHD.";
const EXAMPLE_OUT: &str = "[\"Anna Lind\", \"Storgatan 3\", \"lgh 12\", \"Concerta\", \"ADHD\"]";

pub struct LlmDetector {
    qwen: Qwen,
}

impl LlmDetector {
    /// `_tokenizer_path` is unused — llama.cpp uses the tokenizer embedded in the GGUF. It is kept
    /// so callers can pass the usual `(model, tokenizer)` path pair.
    pub fn load(gguf_path: &Path, _tokenizer_path: &Path) -> Result<Self> {
        Ok(Self { qwen: Qwen::load(gguf_path)? })
    }

    /// Ask the model for verbatim substrings that should be masked. Greedy decoding (temperature 0)
    /// — low-temp sampling made the quantized model parrot the example instead of reading the text.
    pub fn propose(&self, text: &str) -> Result<Vec<String>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let output = self.qwen.generate(&build_prompt(text), 512, 0.0)?;
        let mut seen = std::collections::HashSet::new();
        Ok(parse_json_strings(&output)
            .into_iter()
            .filter(|t| seen.insert(t.to_lowercase()))
            .take(80)
            .collect())
    }
}

fn build_prompt(text: &str) -> String {
    format!(
        "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n\
         <|im_start|>user\nText: {EXAMPLE_IN}<|im_end|>\n\
         <|im_start|>assistant\n{EXAMPLE_OUT}<|im_end|>\n\
         <|im_start|>user\nText: {text}<|im_end|>\n<|im_start|>assistant\n"
    )
}

/// Pull the proposed substrings from the model output. Tries to parse a clean JSON array first;
/// if that fails (e.g. the output was truncated and is missing its closing `]`), falls back to
/// extracting every quoted string literal so we still recover what the model produced.
fn parse_json_strings(s: &str) -> Vec<String> {
    if let (Some(a), Some(b)) = (s.find('['), s.rfind(']')) {
        if b > a {
            if let Ok(serde_json::Value::Array(arr)) =
                serde_json::from_str::<serde_json::Value>(&s[a..=b])
            {
                let parsed: Vec<String> = arr
                    .iter()
                    .filter_map(|v| {
                        v.as_str()
                            .map(str::to_string)
                            .or_else(|| v.get("text").and_then(|t| t.as_str()).map(str::to_string))
                    })
                    .collect();
                if !parsed.is_empty() {
                    return parsed;
                }
            }
        }
    }

    // Fallback: recover quoted strings from the first '[' onward (tolerates truncation).
    let from = s.find('[').map(|i| &s[i..]).unwrap_or(s);
    JSON_STRING
        .captures_iter(from)
        .map(|c| c[1].replace("\\\"", "\"").replace("\\\\", "\\").trim().to_string())
        .filter(|t| t.chars().count() >= 2)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loads the bundled LLM (now Q8_0) and runs it on the sample text. Run with:
    /// `cargo test --release --lib ai::tests::smoke_llm -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn smoke_llm() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/llm");
        let d = LlmDetector::load(&base.join("model.gguf"), &base.join("tokenizer.json")).unwrap();
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/veckobrev.txt"),
        )
        .unwrap();
        let proposals = d.propose(&text).unwrap();
        println!("AI-FÖRSLAG ({}):\n{proposals:#?}", proposals.len());
        assert!(!proposals.is_empty(), "AI-lagret gav inga förslag");
        // Q8_0 + greedy must read the REAL text, not reproduce the few-shot example.
        assert!(
            !proposals.iter().any(|p| p.contains("Anna Lind") || p.contains("Storgatan")),
            "modellen härmade exemplet i stället för att läsa texten: {proposals:?}"
        );
    }

    #[test]
    fn parses_json_array() {
        let s = "Här: [\"Anna Svensson\", \"Lund\"] klart";
        assert_eq!(parse_json_strings(s), vec!["Anna Svensson", "Lund"]);
    }

    #[test]
    fn parses_truncated_json() {
        // Missing closing ] and an unterminated last item (simulates token-limit truncation).
        let s = "[\"Anna Svensson\", \"Lund\", \"Concerta";
        let v = parse_json_strings(s);
        assert!(v.contains(&"Anna Svensson".to_string()));
        assert!(v.contains(&"Lund".to_string()));
    }
}
