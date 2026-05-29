//! Merge transcription segments with diarisation turns into speaker-attributed utterances.
//!
//! Each whisper segment is assigned the speaker whose turns overlap it the most in time. Speaker
//! cluster indices are mapped to stable, first-appearance labels "TALARE_1", "TALARE_2", … so the
//! ordering is intuitive in the UI regardless of the diariser's internal numbering.

use std::collections::HashMap;

use crate::diarize::SpeakerTurn;
use crate::transcribe::RawSegment;
use crate::transcript::Utterance;

/// Build utterances without speaker info (diarisation off).
pub fn without_speakers(segments: Vec<RawSegment>) -> Vec<Utterance> {
    segments
        .into_iter()
        .map(|s| Utterance { start: s.start, end: s.end, speaker: None, text: s.text })
        .collect()
}

/// Build utterances, attributing each segment to its dominant overlapping speaker.
pub fn with_speakers(segments: Vec<RawSegment>, turns: &[SpeakerTurn]) -> Vec<Utterance> {
    // Cluster index -> first-appearance order, walking segments left to right.
    let mut order: HashMap<usize, usize> = HashMap::new();
    let mut next = 1usize;

    segments
        .into_iter()
        .map(|seg| {
            let speaker = dominant_speaker(seg.start, seg.end, turns).map(|cluster| {
                let n = *order.entry(cluster).or_insert_with(|| {
                    let v = next;
                    next += 1;
                    v
                });
                format!("TALARE_{n}")
            });
            Utterance { start: seg.start, end: seg.end, speaker, text: seg.text }
        })
        .collect()
}

/// The cluster index with the greatest temporal overlap of `[start, end]`, if any.
fn dominant_speaker(start: f64, end: f64, turns: &[SpeakerTurn]) -> Option<usize> {
    let mut acc: HashMap<usize, f64> = HashMap::new();
    for t in turns {
        let lo = start.max(t.start);
        let hi = end.min(t.end);
        if hi > lo {
            *acc.entry(t.speaker).or_insert(0.0) += hi - lo;
        }
    }
    acc.into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(speaker, _)| speaker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_dominant_overlap_and_relabels() {
        let segs = vec![
            RawSegment { start: 0.0, end: 2.0, text: "ett".into() },
            RawSegment { start: 2.0, end: 4.0, text: "två".into() },
        ];
        // Diariser numbered them 5 and 2; we expect TALARE_1 then TALARE_2 by appearance.
        let turns = vec![
            SpeakerTurn { start: 0.0, end: 2.0, speaker: 5 },
            SpeakerTurn { start: 2.0, end: 4.0, speaker: 2 },
        ];
        let utts = with_speakers(segs, &turns);
        assert_eq!(utts[0].speaker.as_deref(), Some("TALARE_1"));
        assert_eq!(utts[1].speaker.as_deref(), Some("TALARE_2"));
    }
}
