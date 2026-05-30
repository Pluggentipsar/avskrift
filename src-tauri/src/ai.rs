//! Optional semantic de-identification layer: a small local LLM (quantized GGUF via candle)
//! that proposes additional verbatim substrings to mask. Its output is treated as just another
//! candidate source that flows through the normal human review — never auto-trusted.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_qwen2::ModelWeights as Qwen2;
use candle_transformers::utils::apply_repeat_penalty;
use once_cell::sync::Lazy;
use regex::Regex;
use tokenizers::Tokenizer;

/// Mild penalty on recently generated tokens + low-temperature sampling to avoid repetition loops
/// without harming the JSON structure. Seeded, so output stays reproducible.
const REPEAT_PENALTY: f32 = 1.15;
const REPEAT_LAST_N: usize = 64;
const TEMPERATURE: f64 = 0.3;
const TOP_P: f64 = 0.9;

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
    model: Mutex<Qwen2>,
    tokenizer: Tokenizer,
    eos: u32,
    device: Device,
}

/// Pick the best candle device for the build. With `--features cuda`/`metal` this returns a GPU
/// device (falling back to CPU if no GPU is present at runtime); otherwise CPU. Mirrors the GPU
/// flags that whisper.cpp uses, so a GPU build accelerates both tal->text and the Qwen LLM layer.
pub(crate) fn best_device() -> Device {
    #[cfg(feature = "cuda")]
    {
        if let Ok(d) = Device::new_cuda(0) {
            return d;
        }
    }
    #[cfg(feature = "metal")]
    {
        if let Ok(d) = Device::new_metal(0) {
            return d;
        }
    }
    Device::Cpu
}

impl LlmDetector {
    pub fn load(gguf_path: &Path, tokenizer_path: &Path) -> Result<Self> {
        let device = best_device();
        let mut file = std::fs::File::open(gguf_path)
            .map_err(|e| anyhow!("kunde inte öppna {}: {e}", gguf_path.display()))?;
        let content = gguf_file::Content::read(&mut file)
            .map_err(|e| anyhow!("kunde inte läsa GGUF: {e}"))?;
        let model = Qwen2::from_gguf(content, &mut file, &device)?;

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow!("kunde inte ladda tokenizer: {e}"))?;
        let eos = *tokenizer
            .get_vocab(true)
            .get("<|im_end|>")
            .ok_or_else(|| anyhow!("tokenizer saknar <|im_end|>"))?;

        Ok(Self { model: Mutex::new(model), tokenizer, eos, device })
    }

    /// Ask the model for verbatim substrings that should be masked.
    pub fn propose(&self, text: &str) -> Result<Vec<String>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let output = self.generate(&build_prompt(text), 512)?;
        let mut seen = std::collections::HashSet::new();
        Ok(parse_json_strings(&output)
            .into_iter()
            .filter(|t| seen.insert(t.to_lowercase()))
            .take(80)
            .collect())
    }

    fn generate(&self, prompt: &str, max_new: usize) -> Result<String> {
        let enc = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| anyhow!("tokenisering misslyckades: {e}"))?;
        let tokens = enc.get_ids().to_vec();

        let mut model = self.model.lock().unwrap();
        let mut lp = LogitsProcessor::new(42, Some(TEMPERATURE), Some(TOP_P));
        let mut generated: Vec<u32> = Vec::new();

        let input = Tensor::new(tokens.as_slice(), &self.device)?.unsqueeze(0)?;
        let mut logits = model.forward(&input, 0)?.squeeze(0)?;

        for i in 0..max_new {
            if !generated.is_empty() {
                let start = generated.len().saturating_sub(REPEAT_LAST_N);
                logits = apply_repeat_penalty(&logits, REPEAT_PENALTY, &generated[start..])?;
            }
            let next = lp.sample(&logits)?;
            if next == self.eos {
                break;
            }
            generated.push(next);
            let input = Tensor::new(&[next], &self.device)?.unsqueeze(0)?;
            logits = model.forward(&input, tokens.len() + i)?.squeeze(0)?;
        }
        self.tokenizer.decode(&generated, true).map_err(|e| anyhow!("avkodning misslyckades: {e}"))
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

    /// Loads the bundled LLM and runs it on the sample text. Run with:
    /// `cargo test --lib ai::tests::smoke_llm -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn smoke_llm() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/llm");
        let d = LlmDetector::load(&base.join("model.gguf"), &base.join("tokenizer.json")).unwrap();
        let text = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/veckobrev.txt"),
        )
        .unwrap();
        let raw = d.generate(&build_prompt(&text), 1280).unwrap();
        println!("RÅSVAR:\n{raw}\n--- slut råsvar ---");
        let proposals = parse_json_strings(&raw);
        println!("AI-FÖRSLAG ({}):\n{proposals:#?}", proposals.len());
        assert!(!proposals.is_empty());
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
