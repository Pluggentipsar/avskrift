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
    if mic.len() < FRAME || reference.len() < FRAME || reference.iter().all(|&x| x == 0.0) {
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
#[cfg(test)]
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

    correlation_delay(&mic[start..start + win], reference, start, MAX_LAG)
}

/// Linear cross-correlation via convolution. Zero padding preserves the old boundary handling;
/// f64 reduces numerical error for weak/near-tied correlation peaks.
fn correlation_delay(window: &[f32], reference: &[f32], start: usize, max_lag: usize) -> isize {
    use rustfft::{num_complex::Complex, FftPlanner};
    let win = window.len();
    let ref_len = win + 2 * max_lag;
    let size = (win + ref_len - 1).next_power_of_two();
    let mut a = vec![Complex::new(0.0f64, 0.0); size];
    let mut b = a.clone();
    for (i, &sample) in window.iter().rev().enumerate() {
        a[i].re = sample as f64;
    }
    for (i, slot) in b[..ref_len].iter_mut().enumerate() {
        let index = start as isize + i as isize - max_lag as isize;
        if index >= 0 {
            slot.re = reference.get(index as usize).copied().unwrap_or(0.0) as f64;
        }
    }
    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(size);
    forward.process(&mut a);
    forward.process(&mut b);
    for (x, y) in a.iter_mut().zip(b) {
        *x *= y;
    }
    planner.plan_fft_inverse(size).process(&mut a);
    let mut best = (f64::NEG_INFINITY, -(max_lag as isize));
    for lag in -(max_lag as isize)..=max_lag as isize {
        let score = a[((win - 1 + max_lag) as isize - lag) as usize].re;
        if score > best.0 {
            best = (score, lag);
        }
    }
    best.1
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

    fn noise(n: usize) -> Vec<f32> {
        let mut seed = 42u32;
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed as f64 / u32::MAX as f64 - 0.5) as f32
            })
            .collect()
    }

    fn direct(window: &[f32], reference: &[f32], start: usize, max_lag: usize) -> isize {
        let mut best = (f32::NEG_INFINITY, -(max_lag as isize));
        for lag in -(max_lag as isize)..=max_lag as isize {
            let mut sum = 0.0;
            for (i, &x) in window.iter().enumerate() {
                let index = start as isize + i as isize - lag;
                if index >= 0 {
                    sum += x * reference.get(index as usize).copied().unwrap_or(0.0);
                }
            }
            if sum > best.0 {
                best = (sum, lag);
            }
        }
        best.1
    }

    #[test]
    fn fft_delay_matches_direct_correlation_at_both_edges_and_for_both_signs() {
        let reference = noise(2000);
        for delay in [-80isize, -23, 0, 37, 80] {
            let mic: Vec<_> = (0..2000)
                .map(|i| {
                    let p = i as isize - delay;
                    if p < 0 {
                        0.0
                    } else {
                        reference.get(p as usize).copied().unwrap_or(0.0) * 0.6
                    }
                })
                .collect();
            for start in [0, 500, 1744] {
                let window = &mic[start..start + 256];
                assert_eq!(correlation_delay(window, &reference, start, 80), direct(window, &reference, start, 80));
                assert_eq!(correlation_delay(window, &reference, start, 80), delay);
            }
        }
    }

    #[test]
    #[ignore]
    fn benchmark_echo_delay() {
        let reference = noise(48_000);
        let window = &reference[1000..33000];
        let before = std::time::Instant::now();
        let expected = direct(window, &reference, 1200, MAX_LAG);
        let direct_ms = before.elapsed().as_secs_f64() * 1000.0;
        let now = std::time::Instant::now();
        let actual = correlation_delay(window, &reference, 1200, MAX_LAG);
        let fft_ms = now.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(actual, expected);
        println!("BENCH echo_delay direct_ms={direct_ms:.2} fft_ms={fft_ms:.2} delay={actual}");
    }

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
