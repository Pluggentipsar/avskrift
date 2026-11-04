//! Merge transcription segments with diarisation turns into speaker-attributed utterances.
//!
//! Each whisper segment is assigned the speaker whose turns overlap it the most in time. Speaker
//! cluster indices are mapped to stable, first-appearance labels "TALARE_1", "TALARE_2", … so the
//! ordering is intuitive in the UI regardless of the diariser's internal numbering.

use std::collections::HashMap;

use crate::diarize::SpeakerTurn;
use crate::transcribe::RawSegment;
use crate::transcript::{Utterance, Word};

/// Convert transcriber words to the serialisable transcript words.
fn words_of(seg: &RawSegment) -> Vec<Word> {
    seg.words.iter().map(|w| Word { start: w.start, end: w.end, text: w.text.clone() }).collect()
}

/// Build utterances without speaker info (diarisation off).
pub fn without_speakers(segments: Vec<RawSegment>) -> Vec<Utterance> {
    segments
        .into_iter()
        .map(|s| Utterance { start: s.start, end: s.end, speaker: None, words: words_of(&s), text: s.text })
        .collect()
}

/// Merge already-labelled utterances from several sources into a single timeline, stably sorted by
/// start time. Used by the meeting feature, where the speaker is known from the capture *source*
/// ("Jag" = mic, "Mötet" = system loopback) rather than from diarisation. Because the sort is
/// stable, callers control ties by ordering the input (push mic utterances before meeting ones to
/// favour "Jag" when two utterances start at the same instant).
pub fn from_labeled(mut utterances: Vec<Utterance>) -> Vec<Utterance> {
    utterances.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    utterances
}

/// Re-attribute the utterances currently labelled `meeting_label` (e.g. "Mötet"), or already split
/// into "TALARE_n" by an earlier run, to diarised speakers "TALARE_1".. by dominant temporal overlap
/// with `turns`; utterances with any other speaker (e.g. "Jag") are left untouched. First-appearance
/// numbering, like [`with_speakers`]. An utterance no turn overlaps goes back to `meeting_label`.
pub fn split_meeting_speakers(
    utterances: Vec<Utterance>,
    turns: &[SpeakerTurn],
    meeting_label: &str,
) -> Vec<Utterance> {
    let mut order: HashMap<usize, usize> = HashMap::new();
    let mut next = 1usize;
    utterances
        .into_iter()
        .map(|mut u| {
            if u.speaker.as_deref().is_some_and(|s| s == meeting_label || s.starts_with("TALARE_")) {
                u.speaker = Some(match dominant_speaker(u.start, u.end, turns) {
                    Some(cluster) => {
                        let n = *order.entry(cluster).or_insert_with(|| {
                            let v = next;
                            next += 1;
                            v
                        });
                        format!("TALARE_{n}")
                    }
                    None => meeting_label.to_string(),
                });
            }
            u
        })
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
            let words = words_of(&seg);
            Utterance { start: seg.start, end: seg.end, speaker, words, text: seg.text }
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
    fn meeting_merge_keeps_contrary_replies_and_echo_candidates() {
        let u=|start,end,speaker:&str,text:&str| Utterance {start,end,speaker:Some(speaker.into()),text:text.into(),words:vec![]};
        let out=from_labeled(vec![u(4.2,6.0,"Jag","Vi ska boka lokalen nu."),u(1.0,4.0,"Mötet","Vi ska inte boka lokalen nu.")]);
        assert_eq!(out.len(),2);assert_eq!(out[1].speaker.as_deref(),Some("Jag"));
        assert_eq!(out[1].text,"Vi ska boka lokalen nu.");
    }

    #[test]
    fn assigns_dominant_overlap_and_relabels() {
        let segs = vec![
            RawSegment { start: 0.0, end: 2.0, text: "ett".into(), words: vec![] },
            RawSegment { start: 2.0, end: 4.0, text: "två".into(), words: vec![] },
        ];
        // Diariser numbered them 5 and 2; we expect TALARE_1 then TALARE_2 by appearance.
        let turns =
            vec![SpeakerTurn { start: 0.0, end: 2.0, speaker: 5 }, SpeakerTurn { start: 2.0, end: 4.0, speaker: 2 }];
        let utts = with_speakers(segs, &turns);
        assert_eq!(utts[0].speaker.as_deref(), Some("TALARE_1"));
        assert_eq!(utts[1].speaker.as_deref(), Some("TALARE_2"));
    }

    #[test]
    fn meeting_split_can_be_rerun_and_leaves_my_voice_alone() {
        let u=|start,end,speaker:&str| Utterance {start,end,speaker:Some(speaker.into()),text:String::new(),words:vec![]};
        let turns=vec![SpeakerTurn{start:0.0,end:2.0,speaker:3},SpeakerTurn{start:4.0,end:6.0,speaker:0}];
        let first=split_meeting_speakers(vec![u(0.0,2.0,"Mötet"),u(2.0,4.0,"Jag"),u(4.0,6.0,"Mötet"),u(7.0,8.0,"Mötet")],&turns,"Mötet");
        let names=|v:&[Utterance]|v.iter().map(|u|u.speaker.clone().unwrap()).collect::<Vec<_>>();
        assert_eq!(names(&first),["TALARE_1","Jag","TALARE_2","Mötet"]);
        // A second run with another speaker count re-splits the earlier TALARE_n, not just "Mötet".
        let again=split_meeting_speakers(first,&[SpeakerTurn{start:0.0,end:8.0,speaker:1}],"Mötet");
        assert_eq!(names(&again),["TALARE_1","Jag","TALARE_1","TALARE_1"]);
    }
}
