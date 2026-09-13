//! Disposable on-disk search cache. Caller holds jobs::WRITES across reconciliation/query
//! or mutation; project JSON is always authoritative. Connections never escape this module.
use super::{Action, ActionRow, Job, JobMeta};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::{collections::{BTreeMap, HashSet}, path::{Path, PathBuf}, sync::Mutex, time::UNIX_EPOCH};

const FILE: &str = ".library-v1.sqlite";
static CHECKED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

fn connect(dir: &Path) -> Result<Connection> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE);
    let db = Connection::open(&path)?;
    db.busy_timeout(std::time::Duration::from_secs(2))?;
    db.execute_batch("PRAGMA cache_size=-8192; PRAGMA secure_delete=ON;
        PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")?;
    let version: i32 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    anyhow::ensure!(version == 0 || version == 3, "Sökindexet behöver återskapas.");
    if version == 0 { db.execute_batch("CREATE TABLE IF NOT EXISTS files(name TEXT PRIMARY KEY, stamp TEXT NOT NULL);
        CREATE TABLE IF NOT EXISTS jobs(id TEXT UNIQUE NOT NULL, meta TEXT NOT NULL,
            paths TEXT NOT NULL, actions TEXT NOT NULL, fields TEXT NOT NULL, updated TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS jobs_updated ON jobs(updated DESC, id);
        CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(body, tokenize='trigram case_sensitive 1', columnsize=0);
        INSERT INTO search(search, rank) VALUES('secure-delete', 1);
        PRAGMA user_version=3;")?; }
    let mut checked = CHECKED.lock().unwrap();
    let checked = checked.get_or_insert_with(HashSet::new);
    if !checked.contains(&path) {
        let health: String = db.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        anyhow::ensure!(health == "ok", "Sökindexet är skadat.");
        db.execute("INSERT INTO search(search) VALUES('integrity-check')", [])?;
        checked.insert(path);
    }
    Ok(db)
}

/// Remove only our known disposable cache files, after every connection has been dropped.
fn discard(dir: &Path) -> Result<()> {
    if let Some(checked) = CHECKED.lock().unwrap().as_mut() { checked.remove(&dir.join(FILE)); }
    for suffix in ["", "-journal", "-wal", "-shm"] {
        match std::fs::remove_file(dir.join(format!("{FILE}{suffix}"))) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn stamp(path: &Path) -> Result<String> {
    let m = std::fs::metadata(path)?;
    stamp_metadata(&m)
}
fn stamp_metadata(m: &std::fs::Metadata) -> Result<String> {
    anyhow::ensure!(m.is_file(), "Ingen projektfil.");
    let modified = m.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
    let created = m.created().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|t| t.as_nanos()).unwrap_or(0);
    Ok(format!("{}:{modified}:{created}", m.len()))
}

fn delete_row(db: &Connection, id: &str) -> Result<()> {
    db.execute("DELETE FROM search WHERE rowid IN (SELECT rowid FROM jobs WHERE id=?1)", [id])?;
    db.execute("DELETE FROM jobs WHERE id=?1", [id])?;
    Ok(())
}

fn fields(job: &Job) -> Vec<String> {
    let mut fields = vec![job.title.to_lowercase(), job.category.to_lowercase(), job.notes.to_lowercase(), job.followup.to_lowercase()];
    fields.extend(super::meeting_fields(job));
    fields.extend(job.summary_draft.iter().chain(job.source_text.iter()).map(|s| s.to_lowercase()));
    for a in &job.actions { fields.extend([a.text.to_lowercase(), a.assignee.to_lowercase()]); }
    for p in &job.participants { fields.extend([p.name.to_lowercase(), p.role.to_lowercase()]); }
    if let Some(t) = &job.transcript { fields.extend(t.utterances.iter().map(|u| u.text.to_lowercase())); }
    fields
}

fn put(db: &Connection, job: &Job) -> Result<()> {
    let fields = fields(job);
    let encoded = serde_json::to_string(&fields)?;
    let old: Option<(i64,String)> = db.query_row("SELECT rowid,fields FROM jobs WHERE id=?1", [&job.id], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
    let paths: Vec<&str> = [job.audio_path.as_deref(),job.mic_wav_path.as_deref(),job.mix_wav_path.as_deref()].into_iter().flatten().collect();
    db.execute("INSERT INTO jobs(id,meta,paths,actions,fields,updated) VALUES(?1,?2,?3,?4,?5,?6)
        ON CONFLICT(id) DO UPDATE SET meta=excluded.meta,paths=excluded.paths,actions=excluded.actions,fields=excluded.fields,updated=excluded.updated",
        params![job.id,serde_json::to_string(&super::meta_of(job))?,serde_json::to_string(&paths)?,serde_json::to_string(&job.actions)?,encoded,job.updated_at])?;
    if old.as_ref().is_none_or(|(_, text)| text != &encoded) {
        let rowid: i64 = db.query_row("SELECT rowid FROM jobs WHERE id=?1", [&job.id], |r| r.get(0))?;
        if old.is_some() { db.execute("DELETE FROM search WHERE rowid=?1", [rowid])?; }
        db.execute("INSERT INTO search(rowid,body) VALUES(?1,?2)",params![rowid,fields.join("\n").replace('\0',"\n")])?;
    }
    Ok(())
}

/// Stat each source; read/decode only new or changed JSON. This also recovers a crash between
/// source-file publication and cache invalidation. Versions are in a subdirectory and excluded.
fn reconcile(db: &mut Connection, dir: &Path) -> Result<()> {
    let mut known: BTreeMap<String,String> = db.prepare("SELECT name,stamp FROM files")?
        .query_map([], |r| Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    let tx = db.transaction()?;
    let mut seen = HashSet::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") { continue; }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else { continue; };
        if super::valid_id(id).is_err() { continue; }
        let metadata = entry.metadata()?;
        if !metadata.is_file() { continue; }
        seen.insert(id.to_string());
        let current = stamp_metadata(&metadata)?;
        if known.remove(id).as_ref() == Some(&current) { continue; }
        // If an external writer is mid-update, do not persist a possibly mixed snapshot.
        let bytes = std::fs::read(&path)?;
        anyhow::ensure!(stamp(&path)? == current, "En projektfil ändrades under inläsningen. Försök igen.");
        #[cfg(test)]
        READS.with(|n| n.set(n.get()+1));
        match serde_json::from_slice::<Job>(&bytes) {
            Ok(job) if job.id == id && job.version <= 2 => put(&tx,&job)?,
            _ => delete_row(&tx,id)?, // Unreadable JSON remains on disk, never an old cached hit.
        }
        tx.execute("INSERT INTO files(name,stamp) VALUES(?1,?2) ON CONFLICT(name) DO UPDATE SET stamp=excluded.stamp",params![id,current])?;
    }
    // A dirty (invalidated) row is absent from files but may still have its old postings.
    let ids: Vec<String> = tx.prepare("SELECT id FROM jobs UNION SELECT name FROM files")?
        .query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    for id in ids.into_iter().filter(|id| !seen.contains(id)) {
        delete_row(&tx,&id)?; tx.execute("DELETE FROM files WHERE name=?1", [&id])?;
    }
    tx.commit()?;
    Ok(())
}

fn with_index<T>(dir: &Path, query: impl Fn(&Connection) -> Result<T>) -> Result<T> {
    let attempt = || { let mut db = connect(dir)?; reconcile(&mut db,dir)?; query(&db) };
    match attempt() {
        Ok(value) => Ok(value),
        Err(_) => { discard(dir)?; attempt() }, // One rebuild, then caller falls back to JSON.
    }
}

/// Invalidate after a successful source save. Do not report a failed project save if only this
/// disposable cache failed. The next query must reconcile before reading any cached result.
pub(super) fn changed(dir: &Path, id: &str, deleted: bool) {
    if !dir.join(FILE).exists() { return; }
    let result = (|| -> Result<()> {
        let mut db = connect(dir)?; let tx = db.transaction()?;
        tx.execute("DELETE FROM files WHERE name=?1", [id])?;
        if deleted { delete_row(&tx,id)?; }
        tx.commit()?; Ok(())
    })();
    if result.is_err() { let _ = discard(dir); }
}

fn meta(encoded: &str, paths: &str) -> Result<JobMeta> {
    let mut meta: JobMeta = serde_json::from_str(encoded)?;
    let paths: Vec<String> = serde_json::from_str(paths)?;
    // Audio may be removed externally without modifying the project JSON.
    meta.audio_bytes = paths.iter().filter_map(|p| std::fs::metadata(p).ok()).map(|m| m.len()).sum();
    Ok(meta)
}

pub(super) fn search(dir: &Path, query: &str) -> Result<Vec<JobMeta>> {
    let query = query.trim().to_lowercase();
    with_index(dir, |db| {
        let mut out = Vec::new();
        if query.is_empty() {
            let mut stmt = db.prepare("SELECT meta,paths FROM jobs ORDER BY updated DESC,id")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? { out.push(meta(&row.get::<_,String>(0)?, &row.get::<_,String>(1)?)?); }
        } else {
            // Quoted MATCH is literal (no query operators). Short queries/NUL use an exact scan
            // of the compact cached fields. Check each field to exclude cross-field matches.
            let use_fts = query.chars().count() >= 3 && !query.contains('\0');
            let sql = if use_fts { "SELECT meta,paths,fields FROM jobs WHERE rowid IN (SELECT rowid FROM search WHERE search MATCH ?1) ORDER BY updated DESC,id" }
                else { "SELECT meta,paths,fields FROM jobs WHERE ?1 IS NOT NULL ORDER BY updated DESC,id" };
            let literal = format!("\"{}\"",query.replace('"',"\"\""));
            let mut stmt = db.prepare(sql)?;
            let mut rows = stmt.query([&literal])?;
            while let Some(row) = rows.next()? {
                let fields: Vec<String> = serde_json::from_str(&row.get::<_,String>(2)?)?;
                if fields.iter().any(|field| field.contains(&query)) {
                    out.push(meta(&row.get::<_,String>(0)?, &row.get::<_,String>(1)?)?);
                }
            }
        }
        Ok(out)
    })
}

pub(super) fn actions(dir: &Path) -> Result<Vec<ActionRow>> {
    with_index(dir, |db| {
        let mut out = Vec::new();
        let mut stmt = db.prepare("SELECT meta,actions FROM jobs WHERE actions!='[]' ORDER BY id")?;
        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let m: JobMeta = serde_json::from_str(&row.get::<_,String>(0)?)?;
            let actions: Vec<Action> = serde_json::from_str(&row.get::<_,String>(1)?)?;
            for (i,a) in actions.into_iter().enumerate() {
                out.push(ActionRow { source:"job".into(),job_id:m.id.clone(),job_title:m.title.clone(),job_type:m.job_type.clone(),category:m.category.clone(),task_id:String::new(),index:i,text:a.text,done:a.done,assignee:a.assignee,due:a.due,created_at:m.created_at.clone(),updated_at:m.updated_at.clone() });
            }
        }
        Ok(out)
    })
}

pub(super) fn rebuild(dir: &Path) -> Result<usize> {
    discard(dir).context("Bibliotekets sökuppgifter kunde inte uppdateras")?;
    Ok(search(dir,"")?.len())
}

#[cfg(test)]
thread_local! { static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64,Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("avskrift-index-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
            std::fs::create_dir_all(&path).unwrap(); Self(path)
        }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = discard(&self.0); let _ = std::fs::remove_dir_all(&self.0); } }
    fn job(id: &str) -> Job {
        serde_json::from_value(serde_json::json!({"version":2,"id":id,"jobType":"meeting","title":"Åsa och Östen","category":"Skola/År 8","createdAt":"2026-09-11","updatedAt":"2026-09-11","notes":"alpha\nbeta 50% _under_ \"citat\" AND OR 😀 abc\u{0000}efter noll","followup":"nästa vecka","sourceText":"ÖVNING i gymnastik","summaryDraft":"Ett beslut","actions":[{"text":"Köp böcker","assignee":"Ängla"}],"participants":[{"name":"Björk","role":"Lärare"}],"transcript":{"language":"sv","model":"small","diarized":false,"utterances":[{"start":0,"end":5,"text":"Mötet diskuterar böcker.","speaker":null},{"start":5,"end":10,"text":"Därefter lunch.","speaker":null}]}})).unwrap()
    }
    fn ids(rows: Vec<JobMeta>) -> Vec<String> { let mut ids: Vec<_> = rows.into_iter().map(|r|r.id).collect(); ids.sort(); ids }
    fn write(dir: &Path, job: &Job) { crate::storage::atomic_write(&super::super::job_path(dir,&job.id),&serde_json::to_vec(job).unwrap()).unwrap(); }

    #[test]
    fn literal_substrings_match_the_original_reader_including_swedish_and_short_queries() {
        let temp=Temp::new(); let dir=&temp.0;
        let first=job("first"); write(dir,&first);
        let mut second=job("second");second.title="alpha".into();second.category="beta".into();second.notes=String::new();write(dir,&second);
        for query in ["", "Å", "ÖS", "åsa", "ÖVNING", "50%", "%", "_", "\"citat\"", "AND", "OR", "*", "😀", "abc\0efter", "efter noll", "alpha\nbeta", "böcker.\nDärefter", "År 8", "nästa", "ängla", "LÄRARE", "nonsens", "  Östen  "] {
            assert_eq!(ids(search(dir,query).unwrap()),ids(super::super::scan_search(dir,query)),"query={query:?}");
        }
    }

    #[test]
    fn warm_queries_and_restart_do_not_read_project_json() {
        let temp=Temp::new();let dir=&temp.0;
        for i in 0..20 { write(dir,&job(&format!("job-{i}"))); }
        READS.with(|n|n.set(0));assert_eq!(search(dir,"").unwrap().len(),20);
        assert_eq!(READS.with(|n|n.get()),20);READS.with(|n|n.set(0));
        for query in ["", "Östen", "å", "missing"] { search(dir,query).unwrap(); }
        actions(dir).unwrap();
        CHECKED.lock().unwrap().as_mut().unwrap().remove(&dir.join(FILE));
        search(dir,"böcker").unwrap();
        assert_eq!(READS.with(|n|n.get()),0);
        let mut changed=job("job-3");changed.title="Extern ändring".into();write(dir,&changed);
        assert_eq!(search(dir,"Extern ändring").unwrap().len(),1);
        assert_eq!(READS.with(|n|n.get()),1);
        std::fs::remove_file(super::super::job_path(dir,"job-3")).unwrap();
        assert!(search(dir,"Extern ändring").unwrap().is_empty());
    }

    #[test]
    fn mutations_versions_folders_actions_and_audio_stay_current() {
        let temp=Temp::new();let dir=&temp.0;
        let mut source=job("project");
        let audio=dir.join("synthetic.wav");std::fs::write(&audio,[0u8;123]).unwrap();source.audio_path=Some(audio.to_string_lossy().into());
        super::super::save(dir,&source).unwrap();
        assert_eq!(search(dir,"").unwrap()[0].audio_bytes,123);
        std::fs::remove_file(audio).unwrap();assert_eq!(search(dir,"").unwrap()[0].audio_bytes,0);
        super::super::checkpoint(dir,"project").unwrap();
        let version=super::super::versions(dir,"project").unwrap().remove(0).id;
        source.title="Ny rubrik".into();source.source_text=Some("Ersättning".into());
        super::super::save(dir,&source).unwrap();assert!(search(dir,"gymnastik").unwrap().is_empty());
        assert_eq!(search(dir,"Ny rubrik").unwrap().len(),1);
        super::super::restore(dir,"project",&version,"2026-09-12".into()).unwrap();
        assert_eq!(search(dir,"gymnastik").unwrap().len(),1);assert!(search(dir,"Ny rubrik").unwrap().is_empty());
        super::super::move_folder(dir,"Skola","Utbildning").unwrap();assert!(search(dir,"Skola").unwrap().is_empty());
        assert_eq!(search(dir,"Utbildning/År 8").unwrap().len(),1);
        let mut autosave=super::super::open(dir,"project").unwrap();autosave.category.clear();
        super::super::save_keeping_category(dir,autosave).unwrap();
        assert_eq!(search(dir,"Utbildning/År 8").unwrap().len(),1);
        super::super::set_job_action(dir,"project",0,Action{extra:Default::default(),text:"Ring biblioteket".into(),done:true,assignee:"Lotta".into(),due:"2026-10-01".into()},"2026-09-13").unwrap();
        let rows=actions(dir).unwrap();assert_eq!(rows[0].text,"Ring biblioteket");assert!(rows[0].done);assert_eq!(rows[0].category,"Utbildning/År 8");
        let meta=search(dir,"Lotta").unwrap().remove(0);assert_eq!(meta.actions_done,1);
        super::super::add_job_action(dir,"project",Action{text:"Särskild uppgift".into(),..Default::default()},"2026-09-14").unwrap();
        assert_eq!(search(dir,"Särskild uppgift").unwrap()[0].actions_total,2);
        super::super::delete_job_action(dir,"project",1,"2026-09-15").unwrap();assert!(search(dir,"Särskild uppgift").unwrap().is_empty());
        super::super::delete(dir,"project").unwrap();
        let db=connect(dir).unwrap();assert_eq!(db.query_row("SELECT count(*) FROM jobs",[],|r|r.get::<_,usize>(0)).unwrap(),0);
        assert_eq!(db.query_row("SELECT count(*) FROM search",[],|r|r.get::<_,usize>(0)).unwrap(),0);drop(db);
        assert!(search(dir,"").unwrap().is_empty());
    }

    #[test]
    fn cache_corruption_rebuild_and_unavailable_cache_preserve_originals() {
        let temp=Temp::new();let dir=&temp.0;write(dir,&job("original"));
        let path=super::super::job_path(dir,"original");let original=std::fs::read(&path).unwrap();
        assert_eq!(search(dir,"").unwrap().len(),1);
        std::fs::write(dir.join(FILE),b"broken sqlite").unwrap();
        assert_eq!(search(dir,"Östen").unwrap().len(),1);
        connect(dir).unwrap().execute_batch("PRAGMA user_version=99").unwrap();
        assert_eq!(search(dir,"").unwrap().len(),1);
        assert_eq!(rebuild(dir).unwrap(),1);assert_eq!(std::fs::read(&path).unwrap(),original);
        discard(dir).unwrap();std::fs::create_dir(dir.join(FILE)).unwrap();
        assert_eq!(super::super::search(dir,"Östen").len(),1); // source fallback
        let mut updated=job("original");updated.title="Sparat utan cache".into();
        super::super::save(dir,&updated).unwrap();assert_eq!(super::super::search(dir,"Sparat utan cache").len(),1);
        assert!(dir.join(FILE).is_dir());
    }

    #[test]
    fn broken_or_mismatched_sources_never_leave_stale_hits() {
        let temp=Temp::new();let dir=&temp.0;write(dir,&job("item"));
        assert_eq!(search(dir,"").unwrap().len(),1);
        std::fs::write(super::super::job_path(dir,"item"),b"broken JSON").unwrap();
        assert!(search(dir,"").unwrap().is_empty());
        let mut wrong=job("other");wrong.title="Fel id".into();
        std::fs::write(super::super::job_path(dir,"item"),serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert!(search(dir,"").unwrap().is_empty());
        write(dir,&job("item"));assert_eq!(search(dir,"").unwrap().len(),1);
    }

    #[test]
    fn concurrent_action_edits_are_not_lost_while_the_library_is_read() {
        let temp=Temp::new();let dir=&temp.0;write(dir,&job("shared"));search(dir,"").unwrap();
        let barrier=std::sync::Arc::new(std::sync::Barrier::new(9));
        let handles: Vec<_> = (0..8).map(|i| {
            let dir=dir.clone();let barrier=barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                super::super::add_job_action(&dir,"shared",Action{text:format!("Ny uppgift {i}"),..Default::default()},"2026-09-12").unwrap();
                assert_eq!(super::super::search(&dir,&format!("Ny uppgift {i}")).len(),1);
            })
        }).collect();
        barrier.wait();for handle in handles {handle.join().unwrap();}
        assert_eq!(super::super::open(dir,"shared").unwrap().actions.len(),9);
        assert_eq!(actions(dir).unwrap().len(),9);
    }

    #[test]
    fn standalone_tasks_survive_concurrent_folder_move_and_additions() {
        let temp=Temp::new();let file=temp.0.join("tasks.json");
        super::super::add_task(&file,super::super::StandaloneTask{id:"seed".into(),category:"Gammal".into(),..Default::default()}).unwrap();
        let barrier=std::sync::Arc::new(std::sync::Barrier::new(10));
        let mut handles:Vec<_>=(0..8).map(|i| {
            let file=file.clone();let barrier=barrier.clone();
            std::thread::spawn(move || {barrier.wait();super::super::add_task(&file,super::super::StandaloneTask{id:format!("new-{i}"),category:"Ny".into(),..Default::default()}).unwrap();})
        }).collect();
        let moving=file.clone();let ready=barrier.clone();
        handles.push(std::thread::spawn(move || {ready.wait();super::super::move_task_folder(&moving,"Gammal","Ny").unwrap();}));
        barrier.wait();for handle in handles {handle.join().unwrap();}
        let tasks=super::super::load_tasks(&file);assert_eq!(tasks.len(),9);assert!(tasks.iter().all(|t|t.category=="Ny"));
    }

    #[test]
    #[ignore]
    fn benchmark_thousand_projects() {
        let temp=Temp::new();let dir=&temp.0;
        for i in 0..1000 {
            let mut j=job(&format!("project-{i}"));
            j.source_text=Some(format!("{} unikfras{i}","Syntetiskt mötesunderlag om böcker och undervisning. ".repeat(250)));
            write(dir,&j);
        }
        let start=std::time::Instant::now();search(dir,"").unwrap();let build=start.elapsed();
        READS.with(|n|n.set(0));let start=std::time::Instant::now();
        for _ in 0..10 { assert_eq!(search(dir,"unikfras713").unwrap().len(),1); }
        let warm=start.elapsed();assert_eq!(READS.with(|n|n.get()),0);
        let start=std::time::Instant::now();
        for _ in 0..10 { assert_eq!(super::super::scan_search(dir,"unikfras713").len(),1); }
        let scan=start.elapsed();
        println!("INDEX synthetic_projects=1000 build_ms={} warm_search_avg_ms={:.2} old_search_avg_ms={:.2} warm_json_reads=0 cache_bytes={}",build.as_millis(),warm.as_secs_f64()*100.0,scan.as_secs_f64()*100.0,std::fs::metadata(dir.join(FILE)).unwrap().len());
    }
}
