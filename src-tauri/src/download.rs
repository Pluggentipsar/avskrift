//! On-demand download of Whisper model files with progress reporting.
//!
//! Streams to a temporary file and renames on success so a cancelled/failed download never leaves a
//! half-written `.bin` that would be mistaken for a usable model.

use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, Context, Result};

/// Download `url` to `dest`, calling `progress(downloaded_bytes, total_bytes)` periodically.
pub fn to_file(url: &str, dest: &Path, progress: &dyn Fn(u64, u64)) -> Result<()> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| anyhow!("nedladdningen kunde inte startas: {e}"))?;

    let total: u64 = resp
        .header("Content-Length")
        .and_then(|h| h.parse().ok())
        .unwrap_or(0);

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp = dest.with_extension("part");
    let mut out = std::fs::File::create(&tmp)
        .with_context(|| format!("kunde inte skapa {}", tmp.display()))?;

    let mut reader = resp.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut done: u64 = 0;
    let mut last_report = 0u64;
    loop {
        let n = reader.read(&mut buf).context("fel under nedladdning")?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut out, &buf[..n]).context("kunde inte skriva modellfilen")?;
        done += n as u64;
        // Report roughly every 2 MB to avoid flooding the UI.
        if done - last_report >= 2 * 1024 * 1024 {
            progress(done, total);
            last_report = done;
        }
    }
    out.sync_all().ok();
    drop(out);

    std::fs::rename(&tmp, dest)
        .with_context(|| format!("kunde inte färdigställa {}", dest.display()))?;
    progress(done, total.max(done));
    Ok(())
}
