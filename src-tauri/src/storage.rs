//! Write beside the destination, flush, then replace it. A failed write leaves the old file intact.
use std::{fs::{self, OpenOptions}, io::Write, path::Path, sync::atomic::{AtomicU64, Ordering}};
use anyhow::{Context, Result};
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("lagringsplats saknas")?;
    fs::create_dir_all(parent)?;
    let name = path.file_name().context("filnamn saknas")?.to_string_lossy();
    let temp = parent.join(format!(".{name}.{}-{}.tmp", std::process::id(), SEQUENCE.fetch_add(1, Ordering::Relaxed)));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() { let _ = fs::remove_file(&temp); }
    result.with_context(|| format!("kunde inte spara {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replaces_complete_file_and_cleans_failed_temporary_write() {
        let dir = std::env::temp_dir().join(format!("avskrift-atomic-{}",std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path=dir.join("job.json");
        atomic_write(&path,b"old").unwrap();
        atomic_write(&path,b"new complete text").unwrap();
        assert_eq!(fs::read(&path).unwrap(),b"new complete text");
        let blocked=dir.join("directory"); fs::create_dir_all(&blocked).unwrap();
        assert!(atomic_write(&blocked,b"cannot replace a directory").is_err());
        assert!(blocked.is_dir());
        assert_eq!(fs::read_dir(&dir).unwrap().count(),2);
        fs::remove_dir_all(dir).unwrap();
    }
}
