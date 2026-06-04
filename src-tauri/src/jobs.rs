//! Auto-saved job history. Each completed job (transcribe / deidentify / summarize) is stored as a
//! JSON file `<id>.json` in the writable app-data `jobs/` dir, listed in the History screen and
//! reopenable. The frontend generates the id + timestamps and owns the data; the backend just
//! persists/lists/loads — it does NOT try to keep multiple jobs live at once (the engine/transcript
//! state stays single-active-job; reopening a job re-hydrates the frontend and re-runs analysis).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

use crate::transcript::Transcript;

/// A persisted job. Inputs + settings + verbatim outputs are stored; the de-identify `AnalyzeResult`
/// is intentionally NOT stored (Span byte-offsets are brittle) — it is recomputed on reopen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub version: u32,
    pub id: String,
    /// "transcribe" | "deidentify" | "summarize".
    pub job_type: String,
    pub title: String,
    /// ISO-8601 strings supplied by the frontend (sortable lexicographically).
    pub created_at: String,
    pub updated_at: String,

    // --- transcript-based work ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<Transcript>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub speaker_labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_path: Option<String>,

    // --- standalone de-identify / summarize input ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,

    // --- de-identify settings (category keys are snake_case strings, matching the frontend) ---
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub enabled: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub terms: Vec<String>,
    #[serde(default)]
    pub use_ai: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected: Vec<usize>,

    // --- summary output + settings (verbatim) ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_draft: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_headings: Option<String>,
}

/// Lightweight listing entry for the History screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobMeta {
    pub id: String,
    pub title: String,
    pub job_type: String,
    pub created_at: String,
    pub updated_at: String,
}

fn job_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// Write (or overwrite) a job file.
pub fn save(dir: &Path, job: &Job) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(job)?;
    std::fs::write(job_path(dir, &job.id), json)?;
    Ok(())
}

/// Load a single job by id.
pub fn open(dir: &Path, id: &str) -> Result<Job> {
    let json = std::fs::read_to_string(job_path(dir, id)).map_err(|e| anyhow!("kunde inte läsa jobbet: {e}"))?;
    serde_json::from_str(&json).map_err(|e| anyhow!("ogiltig jobbfil: {e}"))
}

/// Delete a job file (no error if it's already gone).
pub fn delete(dir: &Path, id: &str) -> Result<()> {
    let p = job_path(dir, id);
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

/// List all jobs, newest first (by `updated_at`). Unreadable/invalid files are skipped.
pub fn list(dir: &Path) -> Vec<JobMeta> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(txt) = std::fs::read_to_string(&p) {
                if let Ok(job) = serde_json::from_str::<Job>(&txt) {
                    out.push(JobMeta {
                        id: job.id,
                        title: job.title,
                        job_type: job.job_type,
                        created_at: job.created_at,
                        updated_at: job.updated_at,
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}
