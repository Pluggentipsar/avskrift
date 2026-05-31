//! Speaker diarisation via sherpa-onnx (`sherpa-rs`): pyannote segmentation + a speaker-embedding
//! model, with agglomerative clustering into speakers.
//!
//! Produces speaker "turns" — time ranges each attributed to a cluster — which `align` then matches
//! against the transcript segments.

use std::path::Path;

use anyhow::{anyhow, Result};

/// A contiguous time range attributed to one speaker cluster.
#[derive(Debug, Clone)]
pub struct SpeakerTurn {
    pub start: f64,
    pub end: f64,
    /// Cluster index (0-based).
    pub speaker: usize,
}

/// Run diarisation over 16 kHz mono `samples`.
///
/// `num_speakers` forces a fixed speaker count when `Some`; `None` lets clustering decide via the
/// distance threshold. The pyannote segmentation + embedding ONNX models come from [`crate::models`].
pub fn diarize(
    segmentation_model: &Path,
    embedding_model: &Path,
    samples: &[f32],
    num_speakers: Option<usize>,
    progress: &dyn Fn(&str),
) -> Result<Vec<SpeakerTurn>> {
    use sherpa_rs::diarize::{Diarize, DiarizeConfig};

    for (p, what) in [(segmentation_model, "segmenterings"), (embedding_model, "embedding")] {
        if !p.exists() {
            return Err(anyhow!("diariserings-{what}modellen saknas: {}", p.display()));
        }
    }

    progress("Förbereder diarisering…");
    let config = DiarizeConfig {
        num_clusters: num_speakers.map(|n| n as i32),
        // Used only when num_clusters is unset; tuned for conversational Swedish audio.
        threshold: Some(0.5),
        min_duration_on: Some(0.3),
        min_duration_off: Some(0.5),
        ..Default::default()
    };

    let mut sd = Diarize::new(segmentation_model, embedding_model, config)
        .map_err(|e| anyhow!("kunde inte initiera diariseringen: {e}"))?;

    progress("Identifierar talare…");
    // sherpa-rs takes an optional progress callback; we don't surface clustering progress.
    let segments = sd
        .compute(samples.to_vec(), None)
        .map_err(|e| anyhow!("diariseringen misslyckades: {e}"))?;

    let mut turns: Vec<SpeakerTurn> = segments
        .into_iter()
        .map(|s| SpeakerTurn {
            start: s.start as f64,
            end: s.end as f64,
            speaker: s.speaker.max(0) as usize,
        })
        .collect();
    turns.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    Ok(turns)
}
