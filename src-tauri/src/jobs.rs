//! Auto-saved job history. Each completed job (transcribe / deidentify / summarize) is stored as a
//! JSON file `<id>.json` in the writable app-data `jobs/` dir, listed in the History screen and
//! reopenable. The frontend generates the id + timestamps and owns the data; the backend just
//! persists/lists/loads — it does NOT try to keep multiple jobs live at once (the engine/transcript
//! state stays single-active-job; reopening restores the saved review without re-running a model).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
static WRITES: Mutex<()> = Mutex::new(());
static TASK_WRITES: Mutex<()> = Mutex::new(());
#[path = "job_index.rs"]
mod index;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::transcript::Transcript;

/// A checklist item in the meeting workspace ("Att göra").
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
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

/// A persisted work with immutable originals and review offsets tied to their exact source text.
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
    /// True while a meeting's transcription is still running in the background, or was interrupted by
    /// a crash/force-quit before it finished. The source audio is saved, so the project can be
    /// re-transcribed from the History view to recover the text.
    #[serde(default)]
    pub transcription_pending: bool,
    /// User-chosen folder/category for grouping in History ("" = uncategorised).
    #[serde(default)]
    pub category: String,

    // --- standalone de-identify / summarize input ---
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,

    // --- de-identify settings (category keys are snake_case strings, matching the frontend) ---
    #[serde(default)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_transcript: Option<Transcript>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_source_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_snapshot: Option<crate::engine::ReviewSnapshot>,
    #[serde(default)]
    pub review_is_document: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_basis: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Lightweight listing entry for the History screen.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobMeta {
    #[serde(default)] pub last_opened: String,
    #[serde(default)] pub pinned: bool,
    #[serde(default)] pub archived: bool,
    #[serde(default)] pub followup: String,
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
    /// True if this meeting's transcription is unfinished (in progress or interrupted) — the list
    /// shows a "re-transcribe" hint and the audio is kept so the text can be recovered.
    pub transcription_pending: bool,
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
        last_opened: job.extra.get("lastOpened").and_then(|v|v.as_str()).unwrap_or("").into(),
        pinned: job.extra.get("pinned").and_then(|v|v.as_bool()).unwrap_or(false),
        archived: job.extra.get("archived").and_then(|v|v.as_bool()).unwrap_or(false),
        followup: job.followup.clone(),
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
        transcription_pending: job.transcription_pending,
    }
}

fn job_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

fn version_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json")).collect()
}

#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub struct VersionMeta { id: String, updated_at: String, title: String }

pub fn versions(dir: &Path, id: &str) -> Result<Vec<VersionMeta>> {
    valid_id(id)?;
    let mut files=version_files(&dir.join("versions").join(id));
    files.sort_by(|a,b| b.cmp(a));
    Ok(files.into_iter().filter_map(|p| {
        let job: Job=serde_json::from_slice(&std::fs::read(&p).ok()?).ok()?;
        Some(VersionMeta{id:p.file_stem()?.to_str()?.into(),updated_at:job.updated_at,title:job.title})
    }).collect())
}

pub fn open_version(dir: &Path, id: &str, version: &str) -> Result<Job> {
    valid_id(id)?; valid_id(version)?;
    let bytes=std::fs::read(dir.join("versions").join(id).join(format!("{version}.json")))?;
    let job: Job=serde_json::from_slice(&bytes)?;
    if job.id != id { anyhow::bail!("versionen tillhör ett annat projekt"); }
    Ok(job)
}

pub fn checkpoint(dir: &Path, id: &str) -> Result<()> {
    let _guard=WRITES.lock().map_err(|_| anyhow!("lagringsfel"))?;
    let job=open(dir,id)?;
    save_inner(dir,&job,true)
}

pub fn restore(dir: &Path, id: &str, version: &str, now: String) -> Result<()> {
    let _guard=WRITES.lock().map_err(|_| anyhow!("lagringsfel"))?;
    let mut job=open_version(dir,id,version)?;
    job.updated_at=now;
    save_inner(dir,&job,true)
}

/// Write (or overwrite) a job file.
#[cfg(test)]
pub fn save(dir: &Path, job: &Job) -> Result<()> {
    let _guard = WRITES.lock().map_err(|_| anyhow!("lagringen är upptagen efter ett fel"))?;
    save_inner(dir, job, false)
}

pub fn save_keeping_category(dir: &Path, mut job: Job) -> Result<()> {
    let _guard = WRITES.lock().map_err(|_| anyhow!("lagringen är upptagen efter ett fel"))?;
    if let Ok(existing) = open(dir,&job.id) {
        if job.category.is_empty() { job.category=existing.category; }
        // A delayed autosave from recording/pending UI must not undo native finalisation.
        if job.transcription_pending && !existing.transcription_pending && existing.transcript.is_some() {
            job.transcript=existing.transcript; job.transcription_pending=false;
            job.mix_wav_path=existing.mix_wav_path; job.speaker_labels=existing.speaker_labels;
            for key in ["meetingWarning","meetingError"] {
                job.extra.insert(key.into(),existing.extra.get(key).cloned().unwrap_or_else(||serde_json::json!("")));
            }
        }
    }
    save_inner(dir,&job,false)
}

/// Serialize read-modify-write operations with saves and index reconciliation.
pub fn edit(dir: &Path, id: &str, change: impl FnOnce(&mut Job) -> Result<()>) -> Result<Job> {
    let _guard = WRITES.lock().map_err(|_| anyhow!("lagringen är upptagen efter ett fel"))?;
    let mut job = open(dir,id)?;
    change(&mut job)?;
    save_inner(dir,&job,false)?;
    Ok(job)
}

fn valid_id(id: &str) -> Result<()> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        anyhow::bail!("ogiltigt projekt-id");
    }
    Ok(())
}

#[cfg(test)]
mod version_tests {
    use super::*;
    #[test]
    fn migration_retains_original_and_versions_and_refuses_corrupt_overwrite() {
        let dir=std::env::temp_dir().join(format!("avskrift-versions-{}",std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let legacy=serde_json::json!({"version":1,"id":"fixture","jobType":"deidentify","title":"Test","createdAt":"2026-09-11","updatedAt":"2026-09-11","sourceText":"Åsa och Östen","futureField":{"keep":true}});
        let bytes=serde_json::to_vec(&legacy).unwrap();
        std::fs::write(job_path(&dir,"fixture"),&bytes).unwrap();
        let mut job:Job=serde_json::from_value(legacy).unwrap();
        job.original_source_text=job.source_text.clone();
        job.source_text=Some("Ändrad text".into());
        save(&dir,&job).unwrap();
        assert_eq!(std::fs::read(dir.join("versions/fixture/legacy.json")).unwrap(),bytes);
        let stored=open(&dir,"fixture").unwrap();
        assert_eq!(stored.version,2);
        assert_eq!(stored.original_source_text.as_deref(),Some("Åsa och Östen"));
        assert_eq!(stored.extra["futureField"]["keep"],true);
        assert!(!versions(&dir,"fixture").unwrap().is_empty());
        restore(&dir,"fixture","legacy","2026-09-12".into()).unwrap();
        assert_eq!(open(&dir,"fixture").unwrap().source_text.as_deref(),Some("Åsa och Östen"));
        assert!(open_version(&dir,"fixture","../other").is_err());
        for _ in 0..33 { checkpoint(&dir,"fixture").unwrap(); }
        assert_eq!(versions(&dir,"fixture").unwrap().len(),31); // thirty plus the migration copy
        let mut meeting=job.clone();meeting.id="pending".into();
        meeting.transcript=Some(serde_json::from_value(serde_json::json!({"language":"sv","model":"test","diarized":false,"utterances":[]})).unwrap());
        save(&dir,&meeting).unwrap();
        assert!(open(&dir,"pending").unwrap().original_transcript.is_none());
        meeting.transcript=Some(serde_json::from_value(serde_json::json!({"language":"sv","model":"test","diarized":false,"utterances":[{"start":0,"end":1,"text":"Första texten"}]})).unwrap());
        save(&dir,&meeting).unwrap();
        meeting.transcript.as_mut().unwrap().utterances[0].text="Ny text".into();
        save(&dir,&meeting).unwrap();
        assert_eq!(open(&dir,"pending").unwrap().original_transcript.unwrap().utterances[0].text,"Första texten");
        std::fs::write(job_path(&dir,"fixture"),b"broken").unwrap();
        assert!(save(&dir,&job).is_err());
        assert_eq!(std::fs::read(job_path(&dir,"fixture")).unwrap(),b"broken");
        delete(&dir,"fixture").unwrap();
        assert!(!dir.join("versions/fixture").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

fn save_inner(dir: &Path, job: &Job, checkpoint: bool) -> Result<()> {
    valid_id(&job.id)?;
    if job.version > 2 { anyhow::bail!("Projektet kräver en nyare version av AVskrift."); }
    if let Some(review) = &job.review_snapshot { review.validate(&review.text)?; }
    std::fs::create_dir_all(dir)?;
    let path = job_path(dir, &job.id);
    let mut next = job.clone();
    if path.exists() {
        // An unreadable project is never replaced by an empty or partial recovery.
        let bytes = std::fs::read(&path)?;
        let old: Job = serde_json::from_slice(&bytes).context("det befintliga projektet kunde inte läsas; filen behålls")?;
        if old.version > 2 { anyhow::bail!("Projektet kräver en nyare version av AVskrift; filen behålls."); }
        let versions = dir.join("versions").join(&job.id);
        if old.version < 2 && !versions.join("legacy.json").exists() {
            crate::storage::atomic_write(&versions.join("legacy.json"), &bytes)?;
        }
        let latest = version_files(&versions).into_iter().filter(|p| p.file_stem().is_some_and(|s| s != "legacy"))
            .filter_map(|p| std::fs::metadata(p).ok()?.modified().ok()).max();
        if checkpoint || latest.is_none_or(|t| t.elapsed().is_ok_and(|d| d.as_secs() >= 60)) {
            let stamp=SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
            crate::storage::atomic_write(&versions.join(format!("{stamp}.json")), &bytes)?;
        }
        next.original_transcript = old.original_transcript.or(old.transcript).filter(|t| !t.utterances.is_empty()).or(next.original_transcript);
        next.original_source_text = old.original_source_text.or_else(|| if old.version < 2 { old.source_text } else { None }).or(next.original_source_text);
        for (key,value) in old.extra { next.extra.entry(key).or_insert(value); }
    }
    if next.original_transcript.as_ref().is_none_or(|t| t.utterances.is_empty()) {
        next.original_transcript = next.transcript.clone().filter(|t| !t.utterances.is_empty());
    }
    next.version = 2;
    crate::storage::atomic_write(&path, &serde_json::to_vec_pretty(&next)?)?;
    index::changed(dir, &next.id, false);
    // Retain the migration copy and the 30 most recent checkpoints.
    let mut files = version_files(&dir.join("versions").join(&job.id));
    files.retain(|p| p.file_stem().is_some_and(|s| s != "legacy"));
    files.sort();
    let excess=files.len().saturating_sub(30);
    for file in files.into_iter().take(excess) { let _=std::fs::remove_file(file); }
    Ok(())
}

/// Load a single job by id.
pub fn open(dir: &Path, id: &str) -> Result<Job> {
    valid_id(id)?;
    let json = std::fs::read_to_string(job_path(dir, id)).map_err(|e| anyhow!("kunde inte läsa jobbet: {e}"))?;
    serde_json::from_str(&json).map_err(|e| anyhow!("ogiltig jobbfil: {e}"))
}

/// Delete a job file (no error if it's already gone).
pub fn delete(dir: &Path, id: &str) -> Result<()> {
    valid_id(id)?;
    let _guard=WRITES.lock().map_err(|_| anyhow!("lagringsfel"))?;
    let versions=dir.join("versions").join(id);
    if versions.exists() { std::fs::remove_dir_all(versions)?; }
    let p = job_path(dir, id);
    if p.exists() {
        std::fs::remove_file(p)?;
    }
    index::changed(dir, id, true);
    Ok(())
}

/// JSON remains authoritative; an unavailable cache falls back to the original reader.
pub fn list(dir: &Path) -> Vec<JobMeta> { search(dir, "") }
pub fn search(dir: &Path, query: &str) -> Vec<JobMeta> {
    let _guard = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    index::search(dir,query).unwrap_or_else(|_| scan_search(dir,query))
}
pub fn rebuild_index(dir: &Path) -> Result<usize> {
    let _guard = WRITES.lock().map_err(|_| anyhow!("Lagringen behöver startas om."))?;
    index::rebuild(dir)
}

/// List all jobs, newest first (by `updated_at`). Unreadable/invalid files are skipped.
fn scan_list(dir: &Path) -> Vec<JobMeta> {
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
fn scan_search(dir: &Path, query: &str) -> Vec<JobMeta> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return scan_list(dir);
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
    let _guard = WRITES.lock().map_err(|_| anyhow!("lagringsfel"))?;
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
            save_inner(dir, &job, false)?;
        }
    }
    Ok(())
}

/// Mirror a folder rename / delete onto free-standing tasks (same prefix rewrite as `move_folder`),
/// so loose åtaganden filed in a folder follow it instead of orphaning into a folder that's gone.
pub fn move_task_folder(file: &Path, from: &str, to: &str) -> Result<()> {
    let _guard = TASK_WRITES.lock().map_err(|_| anyhow!("lagringsfel"))?;
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

fn meeting_fields(job: &Job) -> Vec<String> {
    let mut out=Vec::new();
    if let Some(text)=job.extra.get("agenda").and_then(|v|v.as_str()) {out.push(text.to_lowercase());}
    for key in ["decisions","bookmarks"] {
        if let Some(items)=job.extra.get(key).and_then(|v|v.as_array()) {
            for item in items {if let Some(text)=item.get("text").and_then(|v|v.as_str()){out.push(text.to_lowercase());}}
        }
    }
    out
}
fn job_matches(job: &Job, q: &str) -> bool {
    if meeting_fields(job).iter().any(|s|s.contains(q)) {return true;}
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
fn save_tasks(file: &Path, tasks: &[StandaloneTask]) -> Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::atomic_write(file, &serde_json::to_vec_pretty(tasks)?)?;
    Ok(())
}

/// Flatten every action across all jobs, plus the free-standing task store, into one list for the
/// "Åtaganden" overview. Unreadable/invalid job files are skipped (same tolerance as `list`).
fn scan_actions(jobs_dir: &Path) -> Vec<ActionRow> {
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
    out
}
pub fn all_actions(jobs_dir: &Path, tasks_file: &Path) -> Vec<ActionRow> {
    let _guard = WRITES.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = index::actions(jobs_dir).unwrap_or_else(|_| scan_actions(jobs_dir));
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
pub fn set_job_action(dir: &Path, job_id: &str, index: usize, mut action: Action, updated_at: &str) -> Result<()> {
    edit(dir,job_id,|job| {
        if index >= job.actions.len() { anyhow::bail!("åtgärden finns inte längre"); }
        for (key,value) in &job.actions[index].extra {action.extra.entry(key.clone()).or_insert(value.clone());}
        job.actions[index]=action;job.updated_at=updated_at.to_string();Ok(())
    })?;Ok(())
}

pub fn add_job_action(dir: &Path, job_id: &str, action: Action, updated_at: &str) -> Result<()> {
    edit(dir,job_id,|job| {job.actions.push(action);job.updated_at=updated_at.to_string();Ok(())})?;Ok(())
}

pub fn delete_job_action(dir: &Path, job_id: &str, index: usize, updated_at: &str) -> Result<()> {
    edit(dir,job_id,|job| {
        if index < job.actions.len() {job.actions.remove(index);job.updated_at=updated_at.to_string();}Ok(())
    })?;Ok(())
}

/// Add a free-standing task to the store.
pub fn add_task(file: &Path, task: StandaloneTask) -> Result<()> {
    let _guard = TASK_WRITES.lock().map_err(|_| anyhow!("lagringsfel"))?;
    let mut tasks = load_tasks(file);
    tasks.push(task);
    save_tasks(file, &tasks)
}

/// Replace a free-standing task (matched by id). No-op if the id is gone.
pub fn update_task(file: &Path, task: StandaloneTask) -> Result<()> {
    let _guard = TASK_WRITES.lock().map_err(|_| anyhow!("lagringsfel"))?;
    let mut tasks = load_tasks(file);
    if let Some(slot) = tasks.iter_mut().find(|t| t.id == task.id) {
        *slot = task;
    }
    save_tasks(file, &tasks)
}

/// Delete a free-standing task by id.
pub fn delete_task(file: &Path, id: &str) -> Result<()> {
    let _guard = TASK_WRITES.lock().map_err(|_| anyhow!("lagringsfel"))?;
    let mut tasks = load_tasks(file);
    tasks.retain(|t| t.id != id);
    save_tasks(file, &tasks)
}
