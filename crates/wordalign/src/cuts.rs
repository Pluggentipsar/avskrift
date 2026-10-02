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
    /// Loudest millisecond, for scaling a waveform.
    peak_db: f32,
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
        Self::from_samples(audio.iter().copied(), rate)
    }

    /// Streaming variant: memory is one f32 per millisecond (~14 MB per hour), never the audio.
    pub fn from_samples(samples: impl IntoIterator<Item = f32>, rate: usize) -> Self {
        let step = (rate / 1000).max(1);
        // Energy per 1 ms block, then a 10 ms window (5 blocks each side of the block start).
        let mut blocks: Vec<f32> = Vec::new();
        let (mut acc, mut n) = (0f32, 0usize);
        for x in samples {
            acc += x * x;
            n += 1;
            if n == step {
                blocks.push(acc);
                (acc, n) = (0.0, 0);
            }
        }
        let mut db = Vec::with_capacity(blocks.len());
        let mut window = 0f64;
        let (mut lo, mut hi) = (0usize, 0usize);
        for i in 0..blocks.len() {
            let (want_lo, want_hi) = (i.saturating_sub(5), (i + 5).min(blocks.len()));
            while hi < want_hi {
                window += blocks[hi] as f64;
                hi += 1;
            }
            while lo < want_lo {
                window -= blocks[lo] as f64;
                lo += 1;
            }
            let mean = window.max(0.0) / ((hi - lo) * step).max(1) as f64;
            db.push((10.0 * (mean + 1e-10).log10()) as f32);
        }
        let mut sorted: Vec<f32> = db.iter().step_by(10).copied().collect();
        sorted.sort_by(f32::total_cmp);
        let floor = sorted.get(sorted.len() / 10).copied().unwrap_or(-90.0);
        let peak_db = db.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        Self { db, quiet_db: floor + 12.0, peak_db }
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

    /// Waveform for `[start, end)` seconds: `n` bars of the loudest millisecond in each, scaled
    /// 0..1 from the noise floor to the file's peak (silence is 0, full loudness 1).
    pub fn waveform(&self, start: f64, end: f64, n: usize) -> Vec<f32> {
        let floor = self.quiet_db - 12.0;
        let span = (self.peak_db - floor).max(1.0);
        let (a, b) = ((start.max(0.0) * 1000.0) as usize, ((end * 1000.0) as usize).min(self.db.len()));
        (0..n)
            .map(|i| {
                let lo = a + (b.saturating_sub(a)) * i / n.max(1);
                let hi = (a + (b.saturating_sub(a)) * (i + 1) / n.max(1)).max(lo + 1).min(self.db.len());
                if lo >= hi {
                    return 0.0;
                }
                let max = self.db[lo..hi].iter().copied().fold(f32::NEG_INFINITY, f32::max);
                ((max - floor) / span).clamp(0.0, 1.0)
            })
            .collect()
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
    fn waveform_is_zero_in_silence_and_high_in_sound() {
        let l = Loudness::new(&audio(), 16000);
        let w = l.waveform(0.0, 1.0, 20);
        assert_eq!(w.len(), 20);
        assert!(w[9] < 0.1 && w[10] < 0.1, "{w:?}"); // 0.45-0.55 s is well inside the silence
        assert!(w[2] > 0.8 && w[17] > 0.8, "{w:?}");
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
