//! Meeting summarisation: turn a (possibly long) transcript into structured Swedish minutes using a
//! local Qwen GGUF model via candle.
//!
//! Long transcripts don't fit a small model's effective context, so we use **map-reduce**: split the
//! transcript into chunks, summarise each ("map"), then synthesise the chunk-summaries into the final
//! templated document ("reduce"). This scales to arbitrary length and keeps every model call inside a
//! comfortable window.
//!
//! Output is always presented to the user as an *editable draft* with an "AI-genererat — granska"
//! warning; nothing here is treated as authoritative.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_qwen2::ModelWeights as Qwen2;
use candle_transformers::utils::apply_repeat_penalty;
use tokenizers::Tokenizer;

const REPEAT_PENALTY: f32 = 1.15;
const REPEAT_LAST_N: usize = 64;
const TEMPERATURE: f64 = 0.4;
const TOP_P: f64 = 0.9;

/// Approximate characters per chunk for the "map" stage. ~4 chars/token, so ~6 k chars ≈ 1.5 k
/// tokens of transcript + room for prompt and output within a small model's comfort zone.
const CHUNK_CHARS: usize = 6_000;

/// A summarisation template: a stable id, a label, and the section structure the model is asked to
/// fill in. Kept server-side so the prompt is consistent and auditable.
pub struct Template {
    pub id: &'static str,
    pub label: &'static str,
    /// Instruction describing the desired output structure (markdown headings).
    pub structure: &'static str,
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "protokoll",
        label: "Mötesprotokoll",
        structure: "Strukturera som ett mötesprotokoll med dessa rubriker (utelämna en rubrik om \
            inget relevant sägs):\n\
            ## Deltagare\n## Dagordning\n## Diskussion\n## Beslut\n## Åtgärder (vem – vad – när)\n## Övrigt",
    },
    Template {
        id: "sammanfattning",
        label: "Kort sammanfattning",
        structure: "Skriv:\n## Sammanfattning\n(2–4 meningar)\n## Viktigaste punkter\n(punktlista)",
    },
    Template {
        id: "actions",
        label: "Endast åtgärder & beslut",
        structure: "Lista:\n## Beslut\n(punktlista)\n## Åtgärder\n(punktlista, ange ansvarig och \
            tidsram när det framgår)",
    },
];

pub fn template(id: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.id == id)
}

/// A loaded summarisation model. Distinct from the PII `LlmDetector` so the two can use different
/// models/sampling without interfering.
pub struct Summarizer {
    model: Mutex<Qwen2>,
    tokenizer: Tokenizer,
    eos: u32,
    device: Device,
}

impl Summarizer {
    pub fn load(gguf_path: &Path, tokenizer_path: &Path) -> Result<Self> {
        let device = crate::ai::best_device();
        let mut file = std::fs::File::open(gguf_path)
            .map_err(|e| anyhow!("kunde inte öppna {}: {e}", gguf_path.display()))?;
        let content =
            gguf_file::Content::read(&mut file).map_err(|e| anyhow!("kunde inte läsa GGUF: {e}"))?;
        let model = Qwen2::from_gguf(content, &mut file, &device)?;

        let tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow!("kunde inte ladda tokenizer: {e}"))?;
        let eos = *tokenizer
            .get_vocab(true)
            .get("<|im_end|>")
            .ok_or_else(|| anyhow!("tokenizer saknar <|im_end|>"))?;

        Ok(Self { model: Mutex::new(model), tokenizer, eos, device })
    }

    /// Summarise `transcript_text` into the chosen template. `progress` reports map/reduce phases.
    pub fn summarize(
        &self,
        transcript_text: &str,
        template_id: &str,
        progress: &dyn Fn(&str),
    ) -> Result<String> {
        let tmpl = template(template_id).ok_or_else(|| anyhow!("okänd mall: {template_id}"))?;
        let chunks = split_chunks(transcript_text, CHUNK_CHARS);

        if chunks.len() <= 1 {
            // Short enough to do in one pass — skip the map stage.
            progress("Sammanfattar…");
            return self.generate(&final_prompt(tmpl, transcript_text), 1024);
        }

        // Map: summarise each chunk into neutral bullet notes.
        let mut notes = String::new();
        for (i, chunk) in chunks.iter().enumerate() {
            progress(&format!("Läser del {}/{}…", i + 1, chunks.len()));
            let partial = self.generate(&map_prompt(chunk), 512)?;
            notes.push_str(&format!("\n[Del {}]\n{}\n", i + 1, partial.trim()));
        }

        // Reduce: synthesise the notes into the final templated document.
        progress("Sätter ihop sammanfattningen…");
        self.generate(&final_prompt(tmpl, notes.trim()), 1024)
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

const SYSTEM: &str = "Du är en noggrann svensk mötessekreterare. Sammanfatta ENBART det som faktiskt \
sägs i underlaget. Hitta ALDRIG på beslut, namn, siffror eller åtgärder. Om något är oklart eller \
saknas, skriv inget om det. Skriv koncis, korrekt svenska.";

fn map_prompt(chunk: &str) -> String {
    format!(
        "<|im_start|>system\n{SYSTEM}<|im_end|>\n\
         <|im_start|>user\nSammanfatta nyckelpunkterna i detta utdrag ur ett möte som korta \
         neutrala punkter (beslut, åtgärder, ämnen). Underlag:\n\n{chunk}<|im_end|>\n\
         <|im_start|>assistant\n"
    )
}

fn final_prompt(tmpl: &Template, body: &str) -> String {
    format!(
        "<|im_start|>system\n{SYSTEM}<|im_end|>\n\
         <|im_start|>user\n{}\n\nUnderlag (mötestranskript eller delsammanfattningar):\n\n{body}\
         <|im_end|>\n<|im_start|>assistant\n",
        tmpl.structure
    )
}

/// Split text into chunks of at most `max_chars`, preferring to break on line boundaries so an
/// utterance/turn isn't cut mid-sentence.
fn split_chunks(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        if !cur.is_empty() && cur.len() + line.len() + 1 > max_chars {
            chunks.push(std::mem::take(&mut cur));
        }
        // A single very long line: hard-split it.
        if line.len() > max_chars {
            for piece in line.as_bytes().chunks(max_chars) {
                chunks.push(String::from_utf8_lossy(piece).into_owned());
            }
            continue;
        }
        if !cur.is_empty() {
            cur.push('\n');
        }
        cur.push_str(line);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_have_unique_ids() {
        let mut ids: Vec<&str> = TEMPLATES.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), TEMPLATES.len());
    }

    #[test]
    fn chunking_respects_line_boundaries_and_size() {
        let text = (0..50).map(|i| format!("Rad {i}")).collect::<Vec<_>>().join("\n");
        let chunks = split_chunks(&text, 40);
        assert!(chunks.len() > 1);
        for c in &chunks {
            assert!(c.len() <= 40, "chunk för stor: {}", c.len());
            assert!(!c.starts_with('\n'));
        }
        // Reassembling restores the content (modulo the split newlines).
        assert_eq!(chunks.join("\n").replace('\n', ""), text.replace('\n', ""));
    }

    #[test]
    fn short_text_is_single_chunk() {
        assert_eq!(split_chunks("hej hej", 6000).len(), 1);
    }
}
