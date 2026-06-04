//! Model registry and path resolution.
//!
//! Two model families live side by side:
//!   * **Tal -> text**: KB-Whisper in GGML/GGUF form, one file per size. Selectable at runtime.
//!   * **Diarisering**: a pyannote segmentation ONNX + a speaker-embedding ONNX (sherpa-onnx).
//!   * **Avidentifiering**: KB-BERT NER (ONNX) + Qwen (GGUF) — reused from Avidentifierare.
//!
//! Whisper models are large and there are several sizes, so they are *not* embedded in the
//! installer. The smallest default is bundled; the rest are downloaded on demand into the app's
//! data directory. Diarisation + PII models are small enough to embed as bundle resources.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// One selectable KB-Whisper size.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhisperModelInfo {
    /// Stable id used by the frontend and as the on-disk filename stem, e.g. "kb-whisper-small".
    pub id: String,
    /// Human label, e.g. "Small (snabb, bra balans)".
    pub label: String,
    /// Approximate download size in MB (for the UI).
    pub size_mb: u32,
    /// Whether the .bin file is present on disk right now.
    pub downloaded: bool,
}

/// The five KB-Whisper sizes KBLab publishes, as `(id, label, size_mb, ggml_url)`.
///
/// The URLs point at GGML/whisper.cpp conversions of KB-Whisper. **Verify these against the actual
/// published artefacts before release** (see FINISH.md) — KBLab publishes the PyTorch/CT2 weights,
/// and the GGML files come from a community/own conversion repo.
pub const WHISPER_MODELS: &[(&str, &str, u32, &str)] = &[
    (
        "kb-whisper-tiny",
        "Tiny (q5) — snabbast, lägst kvalitet",
        28,
        "https://huggingface.co/KBLab/kb-whisper-tiny/resolve/main/ggml-model-q5_0.bin",
    ),
    (
        "kb-whisper-base",
        "Base (q5) — snabb",
        53,
        "https://huggingface.co/KBLab/kb-whisper-base/resolve/main/ggml-model-q5_0.bin",
    ),
    (
        "kb-whisper-small",
        "Small (q5) — bra balans (rekommenderad)",
        167,
        "https://huggingface.co/KBLab/kb-whisper-small/resolve/main/ggml-model-q5_0.bin",
    ),
    (
        "kb-whisper-medium",
        "Medium (q5) — högre kvalitet, långsammare",
        514,
        "https://huggingface.co/KBLab/kb-whisper-medium/resolve/main/ggml-model-q5_0.bin",
    ),
    (
        "kb-whisper-large",
        "Large (q5) — bäst kvalitet, kräver kraftig dator",
        1031,
        "https://huggingface.co/KBLab/kb-whisper-large/resolve/main/ggml-model-q5_0.bin",
    ),
];

/// Download URL for a Whisper model id.
pub fn whisper_url(id: &str) -> Option<&'static str> {
    WHISPER_MODELS.iter().find(|(mid, ..)| *mid == id).map(|(.., url)| *url)
}

/// One selectable summarisation model. Larger = better Swedish prose / less hallucination, but
/// slower and a bigger download. These are downloaded on demand into the writable model dir.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryModelInfo {
    pub id: String,
    pub label: String,
    pub size_mb: u32,
    pub downloaded: bool,
}

/// Downloadable summarisation models as `(id, label, size_mb, gguf_url, tokenizer_url)`.
///
/// All Qwen2.5-Instruct (Apache-2.0), GGUF **Q8_0** via bartowski. Q4_K_M was too degraded for these
/// small models — they ignored the instructions (parroted the few-shot example / echoed the input);
/// Q8_0 follows them. The 1.5B mirrors the bundled PII model so summarisation works out of the box;
/// 3B/7B are opt-in for noticeably better minutes. **Verify URLs before release** (see FINISH.md).
pub const SUMMARY_MODELS: &[(&str, &str, u32, &str, &str)] = &[
    (
        "qwen2.5-1.5b",
        "Liten (1,5B) — snabbast, finns redan",
        1570,
        "https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/Qwen2.5-1.5B-Instruct-Q8_0.gguf",
        "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct/resolve/main/tokenizer.json",
    ),
    (
        "qwen2.5-3b",
        "Medel (3B) — bra balans (rekommenderad)",
        3133,
        "https://huggingface.co/bartowski/Qwen2.5-3B-Instruct-GGUF/resolve/main/Qwen2.5-3B-Instruct-Q8_0.gguf",
        "https://huggingface.co/Qwen/Qwen2.5-3B-Instruct/resolve/main/tokenizer.json",
    ),
    (
        "qwen2.5-7b",
        "Stor (7B) — bäst kvalitet, kräver kraftig dator / GPU",
        8100,
        "https://huggingface.co/bartowski/Qwen2.5-7B-Instruct-GGUF/resolve/main/Qwen2.5-7B-Instruct-Q8_0.gguf",
        "https://huggingface.co/Qwen/Qwen2.5-7B-Instruct/resolve/main/tokenizer.json",
    ),
];

/// `(gguf_url, tokenizer_url)` for a summary model id.
pub fn summary_urls(id: &str) -> Option<(&'static str, &'static str)> {
    SUMMARY_MODELS.iter().find(|(mid, ..)| *mid == id).map(|(_, _, _, gguf, tok)| (*gguf, *tok))
}

/// Resolved on-disk locations of every model the app needs.
pub struct ModelPaths {
    /// Directory holding `<id>.bin` Whisper models (app data dir, writable).
    pub whisper_dir: PathBuf,
    /// pyannote segmentation ONNX (bundled resource).
    pub diar_segmentation: PathBuf,
    /// Speaker-embedding ONNX (bundled resource).
    pub diar_embedding: PathBuf,
    // --- PII (reused) ---
    pub ner_model: PathBuf,
    pub ner_tokenizer: PathBuf,
    pub ner_labels: PathBuf,
    pub llm_model: PathBuf,
    pub llm_tokenizer: PathBuf,
    /// Directory holding downloaded summarisation models (`<id>.gguf` + `<id>.tokenizer.json`).
    pub summary_dir: PathBuf,
    /// Directory holding auto-saved job-history files (`<id>.json`, app data dir, writable).
    pub jobs_dir: PathBuf,
    /// Directory holding meeting recordings — two source WAVs per meeting (app data dir, writable).
    pub meetings_dir: PathBuf,
}

impl ModelPaths {
    /// Full path to a Whisper model file by id.
    pub fn whisper_file(&self, id: &str) -> PathBuf {
        self.whisper_dir.join(format!("{id}.bin"))
    }

    /// The catalogue with live `downloaded` flags.
    pub fn whisper_catalogue(&self) -> Vec<WhisperModelInfo> {
        WHISPER_MODELS
            .iter()
            .map(|(id, label, size_mb, _url)| WhisperModelInfo {
                id: (*id).to_string(),
                label: (*label).to_string(),
                size_mb: *size_mb,
                downloaded: self.whisper_file(id).exists(),
            })
            .collect()
    }

    /// GGUF + tokenizer paths for a summary model id. The 1.5B id reuses the bundled PII LLM (so
    /// summarisation works without any download); others live in the writable summary dir.
    pub fn summary_files(&self, id: &str) -> (PathBuf, PathBuf) {
        if id == "qwen2.5-1.5b" && self.llm_model.exists() {
            return (self.llm_model.clone(), self.llm_tokenizer.clone());
        }
        (self.summary_dir.join(format!("{id}.gguf")), self.summary_dir.join(format!("{id}.tokenizer.json")))
    }

    /// True when both files for a summary model are present (or it's the bundled 1.5B).
    pub fn summary_downloaded(&self, id: &str) -> bool {
        let (gguf, tok) = self.summary_files(id);
        gguf.exists() && tok.exists()
    }

    /// The summary-model catalogue with live `downloaded` flags.
    pub fn summary_catalogue(&self) -> Vec<SummaryModelInfo> {
        SUMMARY_MODELS
            .iter()
            .map(|(id, label, size_mb, ..)| SummaryModelInfo {
                id: (*id).to_string(),
                label: (*label).to_string(),
                size_mb: *size_mb,
                downloaded: self.summary_downloaded(id),
            })
            .collect()
    }
}

/// Resolve every model path, preferring bundled resources and falling back to the source tree
/// during `tauri dev`. Whisper models live in the writable app-data dir so they can be downloaded
/// after install.
pub fn resolve(app: &AppHandle) -> ModelPaths {
    let resource_dir = app.path().resource_dir().ok();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));

    // A bundled resource: prefer the installed copy, fall back to the dev source tree.
    let res = |rel: &str| -> PathBuf {
        if let Some(rd) = &resource_dir {
            let p = rd.join("resources").join(rel);
            if p.exists() {
                return p;
            }
        }
        manifest.join("resources").join(rel)
    };

    // Writable per-user dir for downloaded Whisper models.
    let data_dir = app.path().app_data_dir().ok();
    let writable = |sub: &str| -> PathBuf {
        data_dir.as_ref().map(|d| d.join(sub)).unwrap_or_else(|| manifest.join("resources").join(sub))
    };
    let whisper_dir = writable("whisper-models");
    let summary_dir = writable("summary-models");
    let jobs_dir = writable("jobs");
    let meetings_dir = writable("meetings");
    let _ = std::fs::create_dir_all(&whisper_dir);
    let _ = std::fs::create_dir_all(&summary_dir);
    let _ = std::fs::create_dir_all(&jobs_dir);
    let _ = std::fs::create_dir_all(&meetings_dir);

    ModelPaths {
        whisper_dir,
        diar_segmentation: res("diarization/segmentation.onnx"),
        diar_embedding: res("diarization/embedding.onnx"),
        ner_model: res("model/model.onnx"),
        ner_tokenizer: res("model/tokenizer.json"),
        ner_labels: res("model/labels.json"),
        llm_model: res("llm/model.gguf"),
        llm_tokenizer: res("llm/tokenizer.json"),
        summary_dir,
        jobs_dir,
        meetings_dir,
    }
}
