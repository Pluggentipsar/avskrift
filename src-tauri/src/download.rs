//! On-demand download of model files with progress, resume and retries.
//!
//! Uses Windows' own trust store and proxy settings (reqwest with rustls-platform-verifier and the
//! system proxy), like a browser does. A bundled certificate list and no proxy support made
//! downloads fail on networks that inspect HTTPS (antivirus, company firewalls) or need a proxy.
//!
//! Streams to `<dest>.part` and renames on success, so a failed download never leaves a file that
//! looks usable. An interrupted download resumes from the `.part` file if the server still has the
//! same file (`If-Range` with the ETag saved next to it); otherwise it starts over.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use reqwest::{blocking::Client, header, StatusCode};

const ATTEMPTS: u32 = 4;

fn client() -> Result<&'static Client> {
    static CLIENT: OnceCell<Client> = OnceCell::new();
    CLIENT.get_or_try_init(|| {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        Client::builder()
            .user_agent(concat!("Avskrift/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(30))
            // The blocking client applies this per send and per read, not to the whole file
            // (models are up to several gigabytes).
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|e| anyhow!("Nätverksstödet kunde inte startas: {e}"))
    })
}

/// Download `url` to `dest`, calling `progress(downloaded_bytes, total_bytes)` periodically.
pub fn to_file(url: &str, dest: &Path, progress: &dyn Fn(u64, u64)) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let tmp = with_suffix(dest, "part");
    let mut last = None;
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
        }
        match fetch(url, &tmp, progress) {
            Ok(()) => {
                let _ = std::fs::remove_file(with_suffix(dest, "part.etag"));
                std::fs::rename(&tmp, dest).with_context(|| format!("kunde inte färdigställa {}", dest.display()))?;
                return Ok(());
            }
            Err(Failure::Retry(e)) => last = Some(e),
            Err(Failure::Stop(e)) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| anyhow!("Hämtningen misslyckades.")))
}

enum Failure {
    /// Worth another try (dropped connection, timeout, server busy); the `.part` file is kept.
    Retry(anyhow::Error),
    Stop(anyhow::Error),
}

fn fetch(url: &str, tmp: &Path, progress: &dyn Fn(u64, u64)) -> Result<(), Failure> {
    let etag_file = with_suffix(tmp, "etag");
    let have = std::fs::metadata(tmp).map(|m| m.len()).unwrap_or(0);
    let etag = std::fs::read_to_string(&etag_file).ok();
    let mut request = client().map_err(Failure::Stop)?.get(url);
    if have > 0 {
        if let Some(etag) = etag.as_deref() {
            request = request.header(header::RANGE, format!("bytes={have}-")).header(header::IF_RANGE, etag.trim());
        }
    }
    let mut response = request.send().map_err(|e| classify(url, e))?;
    let status = response.status();
    if status == StatusCode::RANGE_NOT_SATISFIABLE {
        // The saved part no longer fits the file on the server: start over.
        let _ = std::fs::remove_file(tmp);
        let _ = std::fs::remove_file(&etag_file);
        return Err(Failure::Retry(anyhow!("Hämtningen behövde börja om.")));
    }
    let resumed = status == StatusCode::PARTIAL_CONTENT;
    if !status.is_success() {
        let message = explain_status(url, status);
        return Err(if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
            Failure::Retry(message)
        } else {
            Failure::Stop(message)
        });
    }
    if let Some(tag) = response.headers().get(header::ETAG).and_then(|v| v.to_str().ok()) {
        let _ = std::fs::write(&etag_file, tag);
    }
    let start = if resumed { have } else { 0 };
    let total = response.content_length().map(|n| n + start).unwrap_or(0);
    let stop = |e: std::io::Error, what: &str| Failure::Stop(anyhow!("{what}: {e}"));
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(!resumed)
        .open(tmp)
        .map_err(|e| stop(e, &format!("kunde inte skapa {}", tmp.display())))?;
    if resumed {
        out.seek(std::io::SeekFrom::End(0)).map_err(|e| stop(e, "kunde inte fortsätta filen"))?;
    }
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = start;
    let mut reported = done;
    progress(done, total);
    loop {
        let n = match response.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => return Err(Failure::Retry(anyhow!("Anslutningen bröts under hämtningen ({e}). Försök igen; hämtningen fortsätter där den slutade."))),
        };
        out.write_all(&buf[..n]).map_err(|e| stop(e, "kunde inte skriva modellfilen (är disken full?)"))?;
        done += n as u64;
        if done - reported >= 2 * 1024 * 1024 {
            progress(done, total);
            reported = done;
        }
    }
    out.sync_all().ok();
    if total > 0 && done < total {
        return Err(Failure::Retry(anyhow!("Hämtningen avbröts efter {} av {} MB. Försök igen; den fortsätter där den slutade.", done >> 20, total >> 20)));
    }
    progress(done, total.max(done));
    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(suffix);
    PathBuf::from(name)
}

fn host(url: &str) -> String {
    reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_else(|| url.to_owned())
}

/// The whole error chain as one lowercase string, for recognising the cause.
fn chain(e: &(dyn std::error::Error + 'static)) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        text.push_str(": ");
        text.push_str(&s.to_string());
        source = s.source();
    }
    text
}

fn classify(url: &str, e: reqwest::Error) -> Failure {
    let detail = chain(&e);
    let lower = detail.to_lowercase();
    let host = host(url);
    let message = if lower.contains("certificate") || lower.contains("unknownissuer") || (lower.contains("cert") && lower.contains("verif")) {
        anyhow!(
            "Säker anslutning till {host} kunde inte upprättas: certifikatet godkändes inte. Det händer när ett antivirusprogram eller en brandvägg granskar krypterad trafik med ett eget certifikat som Windows inte litar på. Prova ett annat nätverk, eller hämta filerna i webbläsaren (Hämta i webbläsaren nedan). ({detail})"
        )
    } else if lower.contains("proxy") {
        anyhow!("Proxyn i Windows nätverksinställningar släppte inte igenom anslutningen till {host}. Kontrollera proxyinställningarna eller hämta filerna i webbläsaren. ({detail})")
    } else if lower.contains("dns") || lower.contains("no such host") || lower.contains("failed to lookup") || lower.contains("name or service") {
        anyhow!("Hittade inte {host}. Kontrollera att datorn är ansluten till internet. ({detail})")
    } else if e.is_timeout() {
        return Failure::Retry(anyhow!("{host} svarade inte i tid. Försök igen om en stund. ({detail})"));
    } else if e.is_connect() {
        anyhow!("Kunde inte ansluta till {host}. Nätverket eller en brandvägg kan blockera adressen; prova ett annat nätverk eller hämta filerna i webbläsaren. ({detail})")
    } else {
        return Failure::Retry(anyhow!("Hämtningen från {host} misslyckades. ({detail})"));
    };
    Failure::Stop(message)
}

fn explain_status(url: &str, status: StatusCode) -> anyhow::Error {
    let host = host(url);
    match status.as_u16() {
        401 | 403 => anyhow!("{host} nekade hämtningen ({status}). Nätverket kan blockera adressen; prova ett annat nätverk eller hämta i webbläsaren."),
        404 => anyhow!("Filen finns inte längre på {host} ({status}). Uppdatera Avskrift eller hämta modellen i webbläsaren."),
        429 => anyhow!("{host} begränsar antalet hämtningar just nu ({status}). Vänta en stund och försök igen."),
        _ if status.is_server_error() => anyhow!("{host} har tillfälliga problem ({status}). Försök igen om en stund."),
        _ => anyhow!("Hämtningen från {host} misslyckades ({status})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn part_files_sit_next_to_the_destination() {
        assert_eq!(with_suffix(Path::new("C:/m/model.bin"), "part"), PathBuf::from("C:/m/model.bin.part"));
        assert_eq!(host("https://huggingface.co/KBLab/x/resolve/main/a.bin"), "huggingface.co");
    }
    /// Network test against Hugging Face (follows its redirects to the file store). Opt-in.
    #[test]
    #[ignore]
    fn downloads_and_resumes_from_hugging_face() {
        let url = std::env::var("AVSKRIFT_DOWNLOAD_TEST_URL")
            .unwrap_or_else(|_| "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct/resolve/main/tokenizer.json".into());
        let url = url.as_str();
        let dir = std::env::temp_dir().join(format!("avskrift-download-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let whole = dir.join("whole.json");
        to_file(url, &whole, &|_, _| {}).unwrap();
        let bytes = std::fs::read(&whole).unwrap();
        assert!(bytes.len() > 1_000_000);
        // An interrupted download: half the file plus the ETag the server gave it.
        let resumed = dir.join("resumed.json");
        std::fs::write(with_suffix(&resumed, "part"), &bytes[..bytes.len() / 2]).unwrap();
        let etag = client().unwrap().head(url).send().unwrap().headers().get(header::ETAG).unwrap().to_str().unwrap().to_owned();
        std::fs::write(with_suffix(&resumed, "part.etag"), etag).unwrap();
        let first = std::sync::Mutex::new(None);
        to_file(url, &resumed, &|done, _| {
            first.lock().unwrap().get_or_insert(done);
        })
        .unwrap();
        assert_eq!(first.into_inner().unwrap(), Some(bytes.len() as u64 / 2), "continued where it stopped");
        assert_eq!(std::fs::read(&resumed).unwrap(), bytes);
        assert!(!with_suffix(&resumed, "part.etag").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn status_messages_name_the_host_and_say_what_to_do() {
        let m = explain_status("https://huggingface.co/a", StatusCode::FORBIDDEN).to_string();
        assert!(m.contains("huggingface.co") && m.contains("webbläsaren"));
        assert!(explain_status("https://x.org/a", StatusCode::TOO_MANY_REQUESTS).to_string().contains("Vänta"));
    }
}
