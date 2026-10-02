//! NeMo 16 kHz / 128-band log-mel features. No Python/runtime assets required.
use anyhow::{ensure, Result};
use rustfft::{num_complex::Complex, FftPlanner};

fn mel(hz: f64) -> f64 {
    if hz < 1000.0 {
        hz / (200.0 / 3.0)
    } else {
        15.0 + (hz / 1000.0).ln() / (6.4_f64.ln() / 27.0)
    }
}
fn hz(mel: f64) -> f64 {
    if mel < 15.0 {
        mel * (200.0 / 3.0)
    } else {
        1000.0 * ((mel - 15.0) * (6.4_f64.ln() / 27.0)).exp()
    }
}

pub fn extract(audio: &[f32]) -> Result<(Vec<f32>, usize, usize)> {
    ensure!(
        (512..=120 * 16000).contains(&audio.len()),
        "Expected 0.032–120 seconds of 16 kHz audio"
    );
    ensure!(audio.iter().all(|x| x.is_finite()), "Non-finite audio");
    let valid = audio.len() / 160;
    let frames = valid + 1;
    let mut emphasized = vec![0.0_f32; audio.len() + 512];
    for (i, &sample) in audio.iter().enumerate() {
        emphasized[i + 256] = sample - 0.97_f32 * if i == 0 { 0.0 } else { audio[i - 1] };
    }
    let edges: Vec<_> = (0..130)
        .map(|i| hz(mel(8000.0) * i as f64 / 129.0))
        .collect();
    let mut banks = vec![0.0_f32; 128 * 257];
    for band in 0..128 {
        for bin in 0..257 {
            let freq = bin as f64 * 16000.0 / 512.0;
            let triangle = ((freq - edges[band]) / (edges[band + 1] - edges[band]))
                .min((edges[band + 2] - freq) / (edges[band + 2] - edges[band + 1]))
                .max(0.0);
            // librosa constructs float32 triangles before applying Slaney normalization.
            banks[band * 257 + bin] =
                (triangle as f32 as f64 * 2.0 / (edges[band + 2] - edges[band])) as f32;
        }
    }
    let mut window = [0.0_f64; 512];
    for i in 0..400 {
        window[i + 56] = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / 399.0).cos();
    }
    let fft = FftPlanner::<f64>::new().plan_fft_forward(512);
    let mut buffer = vec![Complex::new(0.0, 0.0); 512];
    let mut scratch = vec![Complex::new(0.0, 0.0); fft.get_inplace_scratch_len()];
    let mut features = vec![0.0_f32; 128 * frames];
    for frame in 0..valid {
        for i in 0..512 {
            buffer[i] = Complex::new(emphasized[frame * 160 + i] as f64 * window[i], 0.0);
        }
        fft.process_with_scratch(&mut buffer, &mut scratch);
        let mut spectrum = [0.0_f32; 257];
        for i in 0..257 {
            let magnitude = buffer[i].norm() as f32;
            spectrum[i] = magnitude * magnitude;
        }
        for band in 0..128 {
            let power: f32 = (0..257)
                .map(|bin| spectrum[bin] * banks[band * 257 + bin])
                .sum();
            features[band * frames + frame] = (power + 2.0_f32.powi(-24)).ln();
        }
    }
    for band in features.chunks_mut(frames) {
        let mean = band[..valid].iter().sum::<f32>() / valid as f32;
        let variance = band[..valid]
            .iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f32>()
            / (valid - 1) as f32;
        for value in &mut band[..valid] {
            *value = (*value - mean) / (variance.sqrt() + 1e-5);
        }
    }
    ensure!(
        features.iter().all(|x| x.is_finite()),
        "Non-finite features"
    );
    Ok((features, frames, valid))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_audio() {
        assert!(extract(&[]).is_err());
        assert!(extract(&vec![f32::NAN; 512]).is_err());
        assert!(extract(&vec![0.0; 120 * 16000 + 1]).is_err());
    }
    #[test]
    fn silence_is_finite_with_masked_tail() {
        let (values, frames, valid) = extract(&vec![0.0; 16000]).unwrap();
        assert_eq!((frames, valid), (101, 100));
        assert!(values.iter().all(|x| x.is_finite()));
        assert!(values.chunks(frames).all(|band| band[valid] == 0.0));
    }
}
