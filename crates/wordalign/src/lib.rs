//! Precise word timestamps for text-based editing ("Textklipp").
//!
//! A transcript (e.g. from KB-Whisper) is force-aligned against the frame-wise character
//! probabilities of a Swedish wav2vec2 CTC model (KBLab VoxRex, exported to ONNX). Whisper's own
//! word times are only used to choose search windows; the returned times come from the acoustic
//! model. See docs/TEXTKLIPP-PROTOTYP.md and docs/TEXTKLIPP-PLAN.md.

pub mod align;
pub mod cuts;
pub mod model;
pub mod normalize;
pub mod sounds;
pub mod viterbi;

pub use align::{align_words, AlignedWord, InputWord};
pub use cuts::{Loudness, Pause};
pub use model::{Device, Emissions, Emitter};
pub use normalize::Vocab;
pub use sounds::{unclaimed, SoundBlock};

/// Files of the shipped model (fp16 export by model-tools/export-voxrex.py) and their SHA-256.
pub const FILES: [(&str, &str); 2] = [
    ("model.fp16.onnx", "bc35af4d3c7dd95810ac1766ef882cbf6aeb6f42eaeb5e3a8cdb275d931b56b0"),
    ("vocab.json", "1d1b27eb4ed992f4560d4bccbb95e353f6a8a057faca71241dc7a863bc2cebb3"),
];
pub const MODEL_FILE: &str = FILES[0].0;
pub const VOCAB_FILE: &str = FILES[1].0;
/// Download size shown in the model list.
pub const SIZE_MB: u32 = 632;

pub fn hash(path: &std::path::Path) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Model input rate.
pub const RATE: usize = 16_000;
/// wav2vec2 emits one frame per 320 samples (20 ms).
pub const HOP: usize = 320;
pub const FRAME_SECONDS: f64 = HOP as f64 / RATE as f64;
