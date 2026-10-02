//! Precise word timestamps for text-based editing ("Textklipp").
//!
//! A transcript (e.g. from KB-Whisper) is force-aligned against the frame-wise character
//! probabilities of a Swedish wav2vec2 CTC model (KBLab VoxRex, exported to ONNX). Whisper's own
//! word times are only used to choose search windows; the returned times come from the acoustic
//! model. See docs/TEXTKLIPP-PROTOTYP.md and docs/TEXTKLIPP-PLAN.md.

pub mod align;
pub mod model;
pub mod normalize;
pub mod viterbi;

pub use align::{align_words, AlignedWord, InputWord};
pub use model::{Device, Emissions, Emitter};
pub use normalize::Vocab;

/// Model input rate.
pub const RATE: usize = 16_000;
/// wav2vec2 emits one frame per 320 samples (20 ms).
pub const HOP: usize = 320;
pub const FRAME_SECONDS: f64 = HOP as f64 / RATE as f64;
