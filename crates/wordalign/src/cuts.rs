//! Where to cut between two kept/removed items, and where the pauses are.
//!
//! Aligned word spans are CTC peaks: sound continues ~0.3 s past a word's aligned end (median in
//! the 32-min test recording). So a cut is never placed at a word boundary but at the quietest
//! video-frame boundary between the previous item's end and the next item's start.

use serde::Serialize;

/// Short-time level in dB at 1 ms resolution (10 ms RMS window).
pub struct Loudness {
    db: Vec<f32>,
    /// Level below which audio counts as pause: the 10th percentile plus 12 dB.
    pub quiet_db: f32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Pause {
    pub start: f64,
    pub end: f64,
}

/// Minimum length of a pause worth listing (shorter gaps are normal speech rhythm).
const MIN_PAUSE: f64 = 0.15;
/// Half-width of the window a candidate cut is judged by.
const JUDGE_MS: usize = 15;

impl Loudness {
    pub fn new(audio: &[f32], rate: usize) -> Self {
        let step = rate / 1000;
        let win = rate / 100;
        let mut prefix = Vec::with_capacity(audio.len() + 1);
        prefix.push(0f64);
        for &x in audio {
            prefix.push(prefix.last().unwrap() + (x as f64) * (x as f64));
        }
        let db: Vec<f32> = (0..audio.len() / step)
            .map(|i| {
                let c = i * step;
                let (lo, hi) = (c.saturating_sub(win / 2), (c + win / 2).min(audio.len()));
                let mean = (prefix[hi] - prefix[lo]) / (hi - lo).max(1) as f64;
                (10.0 * (mean + 1e-10).log10()) as f32
            })
            .collect();
        let mut sorted: Vec<f32> = db.iter().step_by(10).copied().collect();
        sorted.sort_by(f32::total_cmp);
        let floor = sorted.get(sorted.len() / 10).copied().unwrap_or(-90.0);
        Self { db, quiet_db: floor + 12.0 }
    }

    fn level(&self, t: f64) -> f32 {
        let i = ((t * 1000.0).round().max(0.0) as usize).min(self.db.len().saturating_sub(1));
        let (lo, hi) = (i.saturating_sub(JUDGE_MS), (i + JUDGE_MS + 1).min(self.db.len()));
        self.db[lo..hi].iter().sum::<f32>() / (hi - lo).max(1) as f32
    }

    /// Quietest frame boundary (1/fps grid) in `[lo, hi]`; if none fits, the frame boundary
    /// nearest the quietest millisecond. Video and audio are both cut there, keeping sync.
    pub fn cut_point(&self, fps: f64, lo: f64, hi: f64) -> f64 {
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        let (first, last) = ((lo * fps).ceil() as i64, (hi * fps).floor() as i64);
        if first <= last {
            return (first..=last).map(|f| f as f64 / fps).min_by(|a, b| self.level(*a).total_cmp(&self.level(*b))).unwrap();
        }
        let quietest = (((lo * 1000.0) as i64)..=((hi * 1000.0) as i64))
            .map(|ms| ms as f64 / 1000.0)
            .min_by(|a, b| self.level(*a).total_cmp(&self.level(*b)))
            .unwrap_or(lo);
        (quietest * fps).round() / fps
    }

    /// Quiet stretches of at least 150 ms inside the gaps `(end of item, start of next item)`.
    pub fn pauses(&self, gaps: impl IntoIterator<Item = (f64, f64)>) -> Vec<Pause> {
        let mut out = Vec::new();
        for (a, b) in gaps {
            let (lo, hi) = ((a * 1000.0) as usize, ((b * 1000.0) as usize).min(self.db.len()));
            let mut start = None;
            for i in lo..=hi {
                let quiet = i < hi && self.db[i] < self.quiet_db;
                match (quiet, start) {
                    (true, None) => start = Some(i),
                    (false, Some(s)) => {
                        if (i - s) as f64 / 1000.0 >= MIN_PAUSE {
                            out.push(Pause { start: s as f64 / 1000.0, end: i as f64 / 1000.0 });
                        }
                        start = None;
                    }
                    _ => {}
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 s at 16 kHz: tone, silence 0.40–0.60 s, tone.
    fn audio() -> Vec<f32> {
        (0..16000).map(|i| if (6400..9600).contains(&i) { 0.0 } else { (i as f32 * 0.3).sin() * 0.5 }).collect()
    }

    #[test]
    fn cut_lands_in_silence_on_a_frame_boundary() {
        let l = Loudness::new(&audio(), 16000);
        let t = l.cut_point(25.0, 0.30, 0.70);
        assert!((0.44..=0.56).contains(&t), "{t}");
        assert!(((t * 25.0) - (t * 25.0).round()).abs() < 1e-9);
    }

    #[test]
    fn pauses_are_found_inside_gaps_only() {
        let l = Loudness::new(&audio(), 16000);
        let p = l.pauses([(0.35, 0.65)]);
        assert_eq!(p.len(), 1);
        assert!(p[0].start >= 0.39 && p[0].start <= 0.41 && p[0].end >= 0.59 && p[0].end <= 0.61, "{p:?}");
        assert!(l.pauses([(0.1, 0.3)]).is_empty());
    }
}
