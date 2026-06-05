//! Acoustic echo cancellation for meeting capture.
//!
//! Without headphones the microphone ("Jag") also records the meeting audio coming out of the
//! speakers ("Mötet"), so the other person bleeds into the mic stream. This removes that echo from
//! the mic by adaptively modelling the echo path from the system/loopback signal (the reference —
//! what was actually played) and subtracting it. It runs offline at re-transcribe time on the two
//! 16 kHz source streams, so there is no real-time/threading risk and the original recordings are
//! never modified.
//!
//! Pipeline: (1) estimate the bulk delay between the two streams by cross-correlation, then (2) a
//! normalised LMS (NLMS) adaptive FIR models the residual echo path and subtracts it. A final
//! "do no harm" check keeps the result only if it actually reduced energy while the reference was
//! active (i.e. removed echo) without collapsing or blowing up — otherwise the untouched mic is
//! returned, so transcription can never be made worse than before.

const TAPS: usize = 512; // ~32 ms echo path after delay alignment, at 16 kHz
const MU: f32 = 0.4; // NLMS step size (0 < MU < 2 for stability)
const EPS: f32 = 1e-6;
const MAX_LAG: usize = 8_000; // ±0.5 s delay search at 16 kHz
const FRAME: usize = 320; // 20 ms, for the activity/energy guard

/// Remove the echo of `reference` (system/loopback audio) from `mic`. Both are mono 16 kHz and
/// nominally aligned at t = 0. Returns a cleaned mic of the same length, or the original mic if
/// cancellation did not clearly help.
pub fn cancel_echo(mic: &[f32], reference: &[f32]) -> Vec<f32> {
    if mic.len() < FRAME || reference.len() < FRAME {
        return mic.to_vec();
    }
    let delay = estimate_delay(mic, reference);
    let cleaned = nlms(mic, reference, delay);

    // Do no harm: compare mic vs cleaned energy in frames where the reference was active (echo
    // present). Use the cleaned signal only if it removed a meaningful amount there and didn't blow
    // up (divergence would *raise* the energy → ratio ≥ 1 → reject).
    let (orig, clean) = active_frame_energy(mic, &cleaned, reference, delay);
    if orig > 0.0 && clean < orig * 0.95 {
        cleaned
    } else {
        mic.to_vec()
    }
}

/// Sum two mono signals into one playback track, padding the shorter with silence and clamping to
/// [-1, 1]. Used to mix the (echo-cleaned) mic with the system audio so a meeting plays back as one
/// track with both voices.
pub fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    let n = a.len().max(b.len());
    (0..n).map(|i| (a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0)).clamp(-1.0, 1.0)).collect()
}

/// Bulk delay (in samples) such that `mic[i]` echoes `reference[i - delay]`, found by cross-correlating
/// the highest-energy ~2 s window of the reference against the mic over ±[`MAX_LAG`].
fn estimate_delay(mic: &[f32], reference: &[f32]) -> isize {
    let n = mic.len().min(reference.len());
    let win = 32_000.min(n);
    if win < FRAME {
        return 0;
    }
    // Find the start of the highest-energy `win`-sample window in the reference (sliding sum).
    let mut energy: f64 = reference[..win].iter().map(|&x| (x * x) as f64).sum();
    let (mut best_e, mut start) = (energy, 0usize);
    for i in win..n {
        energy += (reference[i] * reference[i]) as f64 - (reference[i - win] * reference[i - win]) as f64;
        if energy > best_e {
            best_e = energy;
            start = i + 1 - win;
        }
    }

    let refs = reference.len() as isize;
    let (mut best_corr, mut best_lag) = (f32::MIN, 0isize);
    let max_lag = MAX_LAG as isize;
    let mut lag = -max_lag;
    while lag <= max_lag {
        let mut c = 0f32;
        for i in 0..win {
            let ri = start as isize + i as isize - lag;
            if ri >= 0 && ri < refs {
                c += mic[start + i] * reference[ri as usize];
            }
        }
        if c > best_corr {
            best_corr = c;
            best_lag = lag;
        }
        lag += 1;
    }
    best_lag
}

/// NLMS adaptive FIR: estimate the echo in `mic` from a `TAPS`-long window of `reference` ending at
/// `reference[i - delay]`, subtract it, and adapt. Samples without enough reference are passed through.
fn nlms(mic: &[f32], reference: &[f32], delay: isize) -> Vec<f32> {
    let n = mic.len();
    let refs = reference.len() as isize;
    let mut w = vec![0f32; TAPS];
    let mut out = vec![0f32; n];
    let taps = TAPS as isize;
    for i in 0..n {
        let base = i as isize - delay - (taps - 1);
        if base < 0 || base + taps > refs {
            out[i] = mic[i];
            continue;
        }
        let xs = &reference[base as usize..base as usize + TAPS];
        let mut y = 0f32;
        let mut pw = 0f32;
        for k in 0..TAPS {
            y += w[k] * xs[k];
            pw += xs[k] * xs[k];
        }
        let e = mic[i] - y;
        out[i] = e;
        let g = MU * e / (pw + EPS);
        for k in 0..TAPS {
            w[k] += g * xs[k];
        }
    }
    out
}

/// Sum of `mic²` and `cleaned²` over 20 ms frames where the (delay-aligned) reference was active —
/// i.e. where echo could be present. Used by the do-no-harm guard.
fn active_frame_energy(mic: &[f32], cleaned: &[f32], reference: &[f32], delay: isize) -> (f64, f64) {
    let ref_mean: f64 = reference.iter().map(|&x| (x * x) as f64).sum::<f64>() / reference.len().max(1) as f64;
    let thresh = ref_mean * 0.5;
    let refs = reference.len() as isize;
    let (mut om, mut cm) = (0f64, 0f64);
    let mut i = 0;
    while i + FRAME <= mic.len() {
        let rb = i as isize - delay;
        if rb >= 0 && rb + FRAME as isize <= refs {
            let rb = rb as usize;
            let re: f64 = reference[rb..rb + FRAME].iter().map(|&x| (x * x) as f64).sum::<f64>() / FRAME as f64;
            if re > thresh {
                om += mic[i..i + FRAME].iter().map(|&x| (x * x) as f64).sum::<f64>();
                cm += cleaned[i..i + FRAME].iter().map(|&x| (x * x) as f64).sum::<f64>();
            }
        }
        i += FRAME;
    }
    (om, cm)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic delayed+attenuated echo of a reference should be largely cancelled.
    #[test]
    fn cancels_synthetic_echo() {
        let n = 16_000; // 1 s
        let delay = 40usize; // 2.5 ms
                             // Reference: a couple of tones (the "far end").
        let reference: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f32 / 16_000.0;
                0.5 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                    + 0.3 * (2.0 * std::f32::consts::PI * 480.0 * t).sin()
            })
            .collect();
        // Mic: just the echo (attenuated, delayed) — no near-end speech.
        let mut mic = vec![0f32; n];
        for i in delay..n {
            mic[i] = 0.6 * reference[i - delay];
        }
        let cleaned = cancel_echo(&mic, &reference);
        let energy = |s: &[f32]| s.iter().map(|&x| (x * x) as f64).sum::<f64>();
        // After convergence the residual (second half) should be much quieter than the input echo.
        let e_in = energy(&mic[n / 2..]);
        let e_out = energy(&cleaned[n / 2..]);
        assert!(e_out < e_in * 0.25, "echo not cancelled: {e_in} -> {e_out}");
    }

    /// With no reference echo present, the mic (near-end only) must be returned unchanged.
    #[test]
    fn keeps_near_end_when_no_echo() {
        let n = 16_000;
        let reference = vec![0f32; n]; // silent far-end → nothing to cancel
        let mic: Vec<f32> = (0..n).map(|i| 0.4 * (i as f32 / 50.0).sin()).collect();
        let cleaned = cancel_echo(&mic, &reference);
        assert_eq!(cleaned, mic);
    }
}
