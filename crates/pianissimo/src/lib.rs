//! Local CPU inference for Klang's pinned Swedish Pianissimo ONNX export.
pub mod encoder;
pub mod features;
pub mod model;
pub mod segments;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub const ID: &str = "pianissimo-sv";
/// Klang's own ONNX export (int8 with SmoothQuant, local attention kept), pinned to a revision.
pub const REPO: &str = "KlangAI/pianissimo-sv-onnx";
pub const REVISION: &str = "63730c6021234f26b9bbae9a07a04fec39e7a52e";
pub const FILES: &[(&str, &str)] = &[
    (
        "encoder-model.int8.onnx",
        "13288a5f4f009bf6bf8a3260044d098f6922e8caf1c564521d4962fa5749f66c",
    ),
    (
        "decoder_joint-model.int8.onnx",
        "0f9213242acd8874f2717c5e3cdb888d4e7673ddf3abb9ee3a2fd2eaf0acdd03",
    ),
    (
        "vocab.txt",
        "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
    ),
    (
        "config.json",
        "f19eee59d2ba995f6d5cdb164e30dd64d2a8b7ed0ce9f2198ab68a5467445694",
    ),
];
pub fn hash(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub fn verify(dir: &Path) -> Result<()> {
    for (file, expected) in FILES {
        anyhow::ensure!(
            hash(&dir.join(file))? == *expected,
            "Felaktig modellfil: {file}. Hämta Pianissimo igen."
        );
    }
    Ok(())
}

const MARKER: &str = "verified.json";
fn marker() -> serde_json::Value {
    serde_json::json!({ "repo": REPO, "revision": REVISION, "files": FILES })
}
/// Cheap readiness check for model listings: `install` has verified exactly these files.
/// Loading still verifies the checksums.
pub fn ready(dir: &Path) -> bool {
    FILES.iter().all(|(file, _)| dir.join(file).is_file())
        && std::fs::read(dir.join(MARKER))
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .is_some_and(|m| m == marker())
}
/// Record a successful verification. Also removes the encoder cache that the earlier
/// community export needed (`prepared/`, `preparing-v1/`, about 1 GB); it is never used now.
pub fn mark_verified(dir: &Path) -> Result<()> {
    verify(dir)?;
    for old in ["prepared", "preparing-v1"] {
        let path = dir.join(old);
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        }
    }
    std::fs::write(dir.join(MARKER), serde_json::to_vec_pretty(&marker())?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ready_needs_marker_for_current_files() {
        let dir = std::env::temp_dir().join(format!("pianissimo-ready-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (file, _) in FILES {
            std::fs::write(dir.join(file), b"x").unwrap();
        }
        assert!(!ready(&dir));
        std::fs::write(dir.join(MARKER), b"{\"revision\":\"old\"}").unwrap();
        assert!(!ready(&dir));
        std::fs::write(dir.join(MARKER), serde_json::to_vec(&marker()).unwrap()).unwrap();
        assert!(ready(&dir));
        assert!(
            mark_verified(&dir).is_err(),
            "wrong checksums must not be marked"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
