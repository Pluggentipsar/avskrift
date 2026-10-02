//! Local CPU inference for the pinned Swedish Pianissimo ONNX export.
pub mod encoder;
pub mod features;
pub mod model;
pub mod prepare;
pub mod segments;
use anyhow::Result;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub const ID: &str = "pianissimo-sv";
pub const REVISION: &str = "72c38267654dadd538bceac7a851de00fb55f11a";
pub const FILES: &[(&str, &str)] = &[
    (
        "encoder-model.int8.onnx",
        "8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2",
    ),
    (
        "decoder_joint-model.int8.onnx",
        "2fb4ef1c1e28839aef70e74a3a2737afdc7460afa1e640ce1c4f9bf9ceadcb51",
    ),
    (
        "vocab.txt",
        "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
    ),
    (
        "config.json",
        "666a05a8b9442a5f084df5915a89b050fed24b66498a04041bca7c951923ad66",
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
