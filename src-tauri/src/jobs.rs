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

/// A checklist item in the meeting workspace ("Att göra").
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub text: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub due: String,
}

/// A free-standing task in the cross-project "Åtaganden" overview — not tied to any meeting/job.
/// All of them live together in a single `standalone-tasks.json` file; the frontend owns the id and
/// timestamps (same convention as jobs).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StandaloneTask {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub assignee: String,
    #[serde(default)]
    pub due: String,
    /// Optional folder path ("/"-separated), so a free-standing task can live in a folder like a job.
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// A meeting participant (name + optional role).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    pub name: String,
    #[serde(default)]
    pub role: String,
}

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
    /// Mic-stream WAV for meetings (kept so re-transcribe works after reopen; deletable to save space).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mic_wav_path: Option<String>,
    /// Mixed playback WAV for meetings (your echo-cleaned mic + the meeting in one track), so you
    /// hear yourself on playback without echo. Regenerated on stop / re-transcribe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mix_wav_path: Option<String>,
    /// User-chosen folder/category for grouping in History ("" = uncategorised).
    #[serde(default)]
    pub category: String,

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

    // --- meeting workspace (notes / participants / actions / follow-up) ---
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub participants: Vec<Participant>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<Action>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub followup: String,
}

/// Lightweight listing entry for the History screen.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobMeta {
    pub id: String,
    pub title: String,
    pub job_type: String,
    pub category: String,
    pub created_at: String,
    pub updated_at: String,
    /// Total bytes of this job's audio files still on disk (0 if none / already removed).
    pub audio_bytes: u64,
    /// Meeting-workspace indicators for the project list (badges).
    pub actions_total: usize,
    pub actions_done: usize,
    pub has_notes: bool,
}

/// One row in the cross-project "Åtaganden" overview — an action lifted out of its job, or a
/// free-standing task. `source` discriminates: "job" rows carry `job_id`/`index` (the index into
/// that job's `actions`), "standalone" rows carry `task_id`. Timestamps are inherited from the
/// owning job/task so the overview can sort by "senast ändrad".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRow {
    pub source: String,
    pub job_id: String,
    pub job_title: String,
    pub job_type: String,
    /// The owning job's folder path ("/"-separated); empty for standalone tasks / root-level jobs.
    pub category: String,
    pub task_id: String,
    pub index: usize,
    pub text: String,
    pub done: bool,
    pub assignee: String,
    pub due: String,
    pub created_at: String,
    pub updated_at: String,
}

fn meta_of(job: &Job) -> JobMeta {
    let audio_bytes = [job.audio_path.as_deref(), job.mic_wav_path.as_deref(), job.mix_wav_path.as_deref()]
        .into_iter()
        .flatten()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    JobMeta {
        id: job.id.clone(),
        title: job.title.clone(),
        job_type: job.job_type.clone(),
        category: job.category.clone(),
        created_at: job.created_at.clone(),
        updated_at: job.updated_at.clone(),
        audio_bytes,
        actions_total: job.actions.len(),
        actions_done: job.actions.iter().filter(|a| a.done).count(),
        has_notes: !job.notes.trim().is_empty(),
    }
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
                    out.push(meta_of(&job));
                }
            }
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

/// Jobs whose title, category, transcript, summary or source text contain `query` (case-insensitive).
/// Empty query returns everything (same as `list`).
pub fn search(dir: &Path, query: &str) -> Vec<JobMeta> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return list(dir);
    }
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(txt) = std::fs::read_to_string(&p) {
                if let Ok(job) = serde_json::from_str::<Job>(&txt) {
                    if job_matches(&job, &q) {
                        out.push(meta_of(&job));
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

/// Rewrite the category prefix `from` → `to` on every job under it — drives folder rename and
/// "delete folder" (collapse into parent: `to` = the parent path, or "" for the root). Paths are
/// "/"-separated; jobs not under `from` are untouched.
pub fn move_folder(dir: &Path, from: &str, to: &str) -> Result<()> {
    let from = from.trim().trim_matches('/');
    let to = to.trim().trim_matches('/');
    if from.is_empty() || from == to {
        return Ok(());
    }
    let with_slash = format!("{from}/");
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(txt) = std::fs::read_to_string(&p) else { continue };
            let Ok(mut job) = serde_json::from_str::<Job>(&txt) else { continue };
            let cat = job.category.trim_matches('/');
            let new = if cat == from {
                to.to_string()
            } else if let Some(rest) = cat.strip_prefix(&with_slash) {
                if to.is_empty() {
                    rest.to_string()
                } else {
                    format!("{to}/{rest}")
                }
            } else {
                continue;
            };
            job.category = new;
            let _ = save(dir, &job);
        }
    }
    Ok(())
}

/// Mirror a folder rename / delete onto free-standing tasks (same prefix rewrite as `move_folder`),
/// so loose åtaganden filed in a folder follow it instead of orphaning into a folder that's gone.
pub fn move_task_folder(file: &Path, from: &str, to: &str) -> Result<()> {
    let from = from.trim().trim_matches('/');
    let to = to.trim().trim_matches('/');
    if from.is_empty() || from == to {
        return Ok(());
    }
    let with_slash = format!("{from}/");
    let mut tasks = load_tasks(file);
    let mut changed = false;
    for t in tasks.iter_mut() {
        let cat = t.category.trim_matches('/');
        let new = if cat == from {
            to.to_string()
        } else if let Some(rest) = cat.strip_prefix(&with_slash) {
            if to.is_empty() {
                rest.to_string()
            } else {
                format!("{to}/{rest}")
            }
        } else {
            continue;
        };
        t.category = new;
        changed = true;
    }
    if changed {
        save_tasks(file, &tasks)?;
    }
    Ok(())
}

fn job_matches(job: &Job, q: &str) -> bool {
    if job.title.to_lowercase().contains(q) || job.category.to_lowercase().contains(q) {
        return true;
    }
    if job.summary_draft.as_deref().is_some_and(|s| s.to_lowercase().contains(q))
        || job.source_text.as_deref().is_some_and(|s| s.to_lowercase().contains(q))
    {
        return true;
    }
    // Meeting workspace: notes / actions / participants / follow-up are searchable too.
    if job.notes.to_lowercase().contains(q) || job.followup.to_lowercase().contains(q) {
        return true;
    }
    if job.actions.iter().any(|a| a.text.to_lowercase().contains(q) || a.assignee.to_lowercase().contains(q)) {
        return true;
    }
    if job.participants.iter().any(|p| p.name.to_lowercase().contains(q) || p.role.to_lowercase().contains(q)) {
        return true;
    }
    job.transcript.as_ref().is_some_and(|t| t.utterances.iter().any(|u| u.text.to_lowercase().contains(q)))
}

// ============================================================================
// Cross-project åtaganden — aggregation + the free-standing task store
// ============================================================================

/// Load the free-standing task store (a missing or invalid file → empty list).
pub fn load_tasks(file: &Path) -> Vec<StandaloneTask> {
    std::fs::read_to_string(file).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Persist the whole free-standing task store.
pub fn save_tasks(file: &Path, tasks: &[StandaloneTask]) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, serde_json::to_string_pretty(tasks)?)?;
    Ok(())
}

/// Flatten every action across all jobs, plus the free-standing task store, into one list for the
/// "Åtaganden" overview. Unreadable/invalid job files are skipped (same tolerance as `list`).
pub fn all_actions(jobs_dir: &Path, tasks_file: &Path) -> Vec<ActionRow> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(jobs_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(txt) = std::fs::read_to_string(&p) else { continue };
            let Ok(job) = serde_json::from_str::<Job>(&txt) else { continue };
            for (i, a) in job.actions.iter().enumerate() {
                out.push(ActionRow {
                    source: "job".into(),
                    job_id: job.id.clone(),
                    job_title: job.title.clone(),
                    job_type: job.job_type.clone(),
                    category: job.category.clone(),
                    task_id: String::new(),
                    index: i,
                    text: a.text.clone(),
                    done: a.done,
                    assignee: a.assignee.clone(),
                    due: a.due.clone(),
                    created_at: job.created_at.clone(),
                    updated_at: job.updated_at.clone(),
                });
            }
        }
    }
    for t in load_tasks(tasks_file) {
        out.push(ActionRow {
            source: "standalone".into(),
            job_id: String::new(),
            job_title: String::new(),
            job_type: String::new(),
            category: t.category,
            task_id: t.id,
            index: 0,
            text: t.text,
            done: t.done,
            assignee: t.assignee,
            due: t.due,
            created_at: t.created_at,
            updated_at: t.updated_at,
        });
    }
    out
}

/// Replace one job's action at `index` (toggle done / edit text·assignee·due). Bumps `updated_at`.
pub fn set_job_action(dir: &Path, job_id: &str, index: usize, action: Action, updated_at: &str) -> Result<()> {
    let mut job = open(dir, job_id)?;
    if index >= job.actions.len() {
        return Err(anyhow!("åtgärden finns inte längre"));
    }
    job.actions[index] = action;
    job.updated_at = updated_at.to_string();
    save(dir, &job)
}

/// Append an action to an existing job. Bumps `updated_at`.
pub fn add_job_action(dir: &Path, job_id: &str, action: Action, updated_at: &str) -> Result<()> {
    let mut job = open(dir, job_id)?;
    job.actions.push(action);
    job.updated_at = updated_at.to_string();
    save(dir, &job)
}

/// Remove one job's action at `index` (no error if it's already gone). Bumps `updated_at`.
pub fn delete_job_action(dir: &Path, job_id: &str, index: usize, updated_at: &str) -> Result<()> {
    let mut job = open(dir, job_id)?;
    if index < job.actions.len() {
        job.actions.remove(index);
        job.updated_at = updated_at.to_string();
        save(dir, &job)?;
    }
    Ok(())
}

/// Add a free-standing task to the store.
pub fn add_task(file: &Path, task: StandaloneTask) -> Result<()> {
    let mut tasks = load_tasks(file);
    tasks.push(task);
    save_tasks(file, &tasks)
}

/// Replace a free-standing task (matched by id). No-op if the id is gone.
pub fn update_task(file: &Path, task: StandaloneTask) -> Result<()> {
    let mut tasks = load_tasks(file);
    if let Some(slot) = tasks.iter_mut().find(|t| t.id == task.id) {
        *slot = task;
    }
    save_tasks(file, &tasks)
}

/// Delete a free-standing task by id.
pub fn delete_task(file: &Path, id: &str) -> Result<()> {
    let mut tasks = load_tasks(file);
    tasks.retain(|t| t.id != id);
    save_tasks(file, &tasks)
}
