//! The transcript data model and its export formats (txt, srt, vtt). Word/.docx export goes through
//! the reused `docio` module.
//!
//! A transcript is a list of utterances. Each utterance has a time range, the recognised text, and
//! (after diarisation) a speaker id like "TALARE_1". Speaker *labels* shown to the user are mapped
//! separately so they can be renamed without touching the model.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One word with absolute timestamps in seconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Word {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Utterance {
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// Speaker id, e.g. "TALARE_1". `None` when diarisation was off.
    pub speaker: Option<String>,
    /// Recognised text (trimmed).
    pub text: String,
    /// Word-level timings (empty unless requested).
    #[serde(default)]
    pub words: Vec<Word>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcript {
    pub utterances: Vec<Utterance>,
    /// Detected/forced language (ISO code), e.g. "sv".
    pub language: String,
    /// Whisper model id used.
    pub model: String,
    /// Whether diarisation was applied.
    pub diarized: bool,
}

impl Transcript {
    /// All utterance texts, in order — the unit of anonymisation (one "paragraph" each).
    pub fn segment_texts(&self) -> Vec<String> {
        self.utterances.iter().map(|u| u.text.clone()).collect()
    }

    /// Distinct speaker ids in first-appearance order.
    pub fn speakers(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for u in &self.utterances {
            if let Some(s) = &u.speaker {
                if !seen.contains(s) {
                    seen.push(s.clone());
                }
            }
        }
        seen
    }

    /// Plain-text transcript. `texts`, when given, replaces each utterance's text in order
    /// (used to render the anonymised version while keeping speaker labels/timestamps).
    /// `labels` maps speaker id -> display name.
    pub fn to_text(&self, texts: Option<&[String]>, labels: &BTreeMap<String, String>) -> String {
        let mut out = String::new();
        let mut last_speaker: Option<&str> = None;
        for (i, u) in self.utterances.iter().enumerate() {
            let body = texts.and_then(|t| t.get(i)).map(String::as_str).unwrap_or(&u.text);
            match &u.speaker {
                Some(sp) => {
                    let name = labels.get(sp).map(String::as_str).unwrap_or(sp);
                    if last_speaker != Some(sp.as_str()) {
                        if !out.is_empty() {
                            out.push('\n');
                        }
                        out.push_str(&format!("{name}: {body}\n"));
                        last_speaker = Some(sp.as_str());
                    } else {
                        out.push_str(&format!("{body}\n"));
                    }
                }
                None => out.push_str(&format!("{body}\n")),
            }
        }
        out
    }

    /// SubRip (.srt) subtitles. Optional `texts` override as in [`to_text`].
    pub fn to_srt(&self, texts: Option<&[String]>, labels: &BTreeMap<String, String>) -> String {
        let mut out = String::new();
        for (i, u) in self.utterances.iter().enumerate() {
            let body = texts.and_then(|t| t.get(i)).map(String::as_str).unwrap_or(&u.text);
            let prefix = speaker_prefix(u, labels);
            out.push_str(&format!("{}\n", i + 1));
            out.push_str(&format!("{} --> {}\n", srt_time(u.start), srt_time(u.end)));
            out.push_str(&format!("{prefix}{body}\n\n"));
        }
        out
    }

    /// WebVTT (.vtt) subtitles. Optional `texts` override as in [`to_text`].
    pub fn to_vtt(&self, texts: Option<&[String]>, labels: &BTreeMap<String, String>) -> String {
        let mut out = String::from("WEBVTT\n\n");
        for (i, u) in self.utterances.iter().enumerate() {
            let body = texts.and_then(|t| t.get(i)).map(String::as_str).unwrap_or(&u.text);
            let prefix = speaker_prefix(u, labels);
            out.push_str(&format!("{} --> {}\n", vtt_time(u.start), vtt_time(u.end)));
            out.push_str(&format!("{prefix}{body}\n\n"));
        }
        out
    }

    /// WebVTT with one cue per **word** (precise karaoke-style timing). Raw text only — used when
    /// word-level timestamps were captured. Speaker, when present, prefixes the first word of a turn.
    pub fn to_vtt_words(&self, labels: &BTreeMap<String, String>) -> String {
        let mut out = String::from("WEBVTT\n\n");
        let mut last_speaker: Option<&str> = None;
        for u in &self.utterances {
            let speaker_changed = u.speaker.as_deref() != last_speaker;
            last_speaker = u.speaker.as_deref();
            for (wi, w) in u.words.iter().enumerate() {
                let prefix = if wi == 0 && speaker_changed {
                    speaker_prefix(u, labels)
                } else {
                    String::new()
                };
                out.push_str(&format!("{} --> {}\n", vtt_time(w.start), vtt_time(w.end)));
                out.push_str(&format!("{prefix}{}\n\n", w.text));
            }
        }
        out
    }

    /// Paragraphs for .docx export: one "Name: text" line per utterance.
    pub fn to_docx_paragraphs(
        &self,
        texts: Option<&[String]>,
        labels: &BTreeMap<String, String>,
    ) -> Vec<String> {
        self.utterances
            .iter()
            .enumerate()
            .map(|(i, u)| {
                let body = texts.and_then(|t| t.get(i)).map(String::as_str).unwrap_or(&u.text);
                format!("{}{}", speaker_prefix(u, labels), body)
            })
            .collect()
    }
}

fn speaker_prefix(u: &Utterance, labels: &BTreeMap<String, String>) -> String {
    match &u.speaker {
        Some(sp) => {
            let name = labels.get(sp).map(String::as_str).unwrap_or(sp);
            format!("{name}: ")
        }
        None => String::new(),
    }
}

fn srt_time(t: f64) -> String {
    let (h, m, s, ms) = hmsms(t);
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

fn vtt_time(t: f64) -> String {
    let (h, m, s, ms) = hmsms(t);
    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
}

fn hmsms(t: f64) -> (u64, u64, u64, u64) {
    let total_ms = (t.max(0.0) * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    (total_s / 3600, (total_s % 3600) / 60, total_s % 60, ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Transcript {
        Transcript {
            language: "sv".into(),
            model: "kb-whisper-small".into(),
            diarized: true,
            utterances: vec![
                Utterance { start: 0.0, end: 2.5, speaker: Some("TALARE_1".into()), text: "Hej.".into(), words: vec![] },
                Utterance { start: 2.5, end: 5.0, speaker: Some("TALARE_2".into()), text: "Hejsan.".into(), words: vec![] },
            ],
        }
    }

    #[test]
    fn srt_has_indices_and_timestamps() {
        let srt = sample().to_srt(None, &BTreeMap::new());
        assert!(srt.contains("1\n00:00:00,000 --> 00:00:02,500"));
        assert!(srt.contains("TALARE_1: Hej."));
    }

    #[test]
    fn vtt_header_and_dotted_time() {
        let vtt = sample().to_vtt(None, &BTreeMap::new());
        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("00:00:02.500 --> 00:00:05.000"));
    }

    #[test]
    fn text_groups_consecutive_speaker() {
        let t = sample().to_text(None, &BTreeMap::new());
        assert!(t.contains("TALARE_1: Hej."));
        assert!(t.contains("TALARE_2: Hejsan."));
    }

    #[test]
    fn anonymised_override_replaces_text() {
        let labels = BTreeMap::new();
        let masked = vec!["[maskerat]".to_string(), "Hejsan.".to_string()];
        let t = sample().to_text(Some(&masked), &labels);
        assert!(t.contains("[maskerat]"));
        assert!(!t.contains("Hej."));
    }
}
