//! Meeting summarisation: turn a (possibly long) transcript into structured Swedish minutes using a
//! local Qwen GGUF model via llama.cpp (see [`crate::llm`]).
//!
//! Qwen2.5 has a 32k context, so a whole meeting transcript usually fits in ONE pass — that is both
//! faster (one model call) and higher quality (nothing lost between chunks). Only transcripts longer
//! than [`CHUNK_CHARS`] fall back to **map-reduce**: summarise each chunk ("map"), then synthesise
//! the chunk-summaries into the final templated document ("reduce").
//!
//! Output is always presented to the user as an *editable draft* with an "AI-genererat — granska"
//! warning; nothing here is treated as authoritative.

use std::path::Path;

use anyhow::Result;

use crate::llm::Qwen;

/// Approximate characters per chunk. Qwen2.5 has a 32k context, so we keep the whole transcript in
/// ONE pass when it fits (~20 k chars ≈ 6 k tokens) — that skips the map stage entirely (1 model
/// call instead of N+1), which is both much faster on CPU and higher quality (nothing is lost
/// across chunk boundaries). Longer transcripts still fall back to map-reduce.
const CHUNK_CHARS: usize = 20_000;

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
/// models without interfering.
pub struct Summarizer {
    qwen: Qwen,
}

impl Summarizer {
    /// `_tokenizer_path` is unused — llama.cpp uses the tokenizer embedded in the GGUF.
    pub fn load(gguf_path: &Path, _tokenizer_path: &Path) -> Result<Self> {
        Ok(Self { qwen: Qwen::load(gguf_path)? })
    }

    /// Summarise `transcript_text` using the chosen structure instruction (from a built-in template
    /// or the user's own agenda/headings). `progress` reports map/reduce phases. Greedy decoding.
    pub fn summarize(&self, transcript_text: &str, structure: &str, progress: &dyn Fn(&str)) -> Result<String> {
        let chunks = split_chunks(transcript_text, CHUNK_CHARS);

        if chunks.len() <= 1 {
            // Fits in one pass — skip the map stage entirely.
            progress("Sammanfattar…");
            return self.qwen.generate(&final_prompt(structure, transcript_text), 1024, 0.0);
        }

        // Map: summarise each chunk into neutral bullet notes.
        let mut notes = String::new();
        for (i, chunk) in chunks.iter().enumerate() {
            progress(&format!("Läser del {}/{}…", i + 1, chunks.len()));
            let partial = self.qwen.generate(&map_prompt(chunk), 512, 0.0)?;
            notes.push_str(&format!("\n[Del {}]\n{}\n", i + 1, partial.trim()));
        }

        // Reduce: synthesise the notes into the final templated document.
        progress("Sätter ihop sammanfattningen…");
        self.qwen.generate(&final_prompt(structure, notes.trim()), 1024, 0.0)
    }

    /// Answer a free-text question **strictly from the transcript** (greedy decoding). For long
    /// transcripts the question is answered per chunk, then the partial answers are woven together.
    pub fn answer(&self, question: &str, transcript_text: &str, progress: &dyn Fn(&str)) -> Result<String> {
        let chunks = split_chunks(transcript_text, CHUNK_CHARS);

        if chunks.len() <= 1 {
            progress("Svarar…");
            return self.qwen.generate(&qa_prompt(question, transcript_text), 512, 0.0);
        }

        let mut partials = String::new();
        for (i, chunk) in chunks.iter().enumerate() {
            progress(&format!("Läser del {}/{}…", i + 1, chunks.len()));
            let a = self.qwen.generate(&qa_prompt(question, chunk), 384, 0.0)?;
            partials.push_str(&format!("\n[Del {}]\n{}\n", i + 1, a.trim()));
        }
        progress("Sammanställer svar…");
        self.qwen.generate(&qa_combine_prompt(question, partials.trim()), 512, 0.0)
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

fn final_prompt(structure: &str, body: &str) -> String {
    format!(
        "<|im_start|>system\n{SYSTEM}<|im_end|>\n\
         <|im_start|>user\n{structure}\n\nUnderlag (mötestranskript eller delsammanfattningar):\
         \n\n{body}<|im_end|>\n<|im_start|>assistant\n"
    )
}

const QA_SYSTEM: &str = "Du svarar på frågor om ett möte. Svara ENBART utifrån transkriptet nedan. \
Hitta ALDRIG på fakta, namn eller siffror. Om svaret inte framgår av transkriptet, säg tydligt att \
det inte framgår. Svara koncist på svenska.";

fn qa_prompt(question: &str, body: &str) -> String {
    format!(
        "<|im_start|>system\n{QA_SYSTEM}<|im_end|>\n\
         <|im_start|>user\nMötestranskript:\n\n{body}\n\n---\nFråga: {question}<|im_end|>\n\
         <|im_start|>assistant\n"
    )
}

fn qa_combine_prompt(question: &str, partials: &str) -> String {
    format!(
        "<|im_start|>system\n{QA_SYSTEM}<|im_end|>\n\
         <|im_start|>user\nDelsvar från olika delar av mötet:\n\n{partials}\n\n---\nVäv ihop \
         delsvaren till ETT sammanhängande svar på frågan: {question}<|im_end|>\n\
         <|im_start|>assistant\n"
    )
}

/// Build a structure instruction from a user-supplied agenda/heading list.
pub fn custom_structure(headings: &str) -> String {
    format!(
        "Strukturera sammanfattningen enligt användarens egen mall nedan. Använd exakt dessa \
         rubriker som ## -rubriker och fyll i relevant innehåll under varje (utelämna en rubrik om \
         inget relevant sägs):\n{headings}"
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
