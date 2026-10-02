//! wav2vec2 CTC emissions via ONNX Runtime, CPU or DirectML.

use std::path::Path;

use anyhow::{anyhow, Result};
use ort::{session::Session, value::Tensor};

use crate::HOP;

/// Seconds of audio per inference window (core), plus context on each side.
const CORE_SAMPLES: usize = 20 * crate::RATE;
const PAD_SAMPLES: usize = crate::RATE;
/// Every window has this exact length, so DirectML compiles one shape only.
const WINDOW: usize = CORE_SAMPLES + 2 * PAD_SAMPLES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Device {
    Cpu,
    /// DirectML (requires the `directml` feature; registration failure is an error, not silent CPU).
    DirectMl,
}

/// Log-probabilities, `frames × vocab`, row-major.
pub struct Emissions {
    pub lp: Vec<f32>,
    pub vocab: usize,
}

impl Emissions {
    pub fn frames(&self) -> usize {
        self.lp.len() / self.vocab
    }
    pub fn rows(&self, from: usize, to: usize) -> &[f32] {
        &self.lp[from * self.vocab..to * self.vocab]
    }
}

pub struct Emitter {
    session: Session,
    pub device: Device,
}

impl Emitter {
    pub fn load(path: &Path, device: Device, threads: usize) -> Result<Self> {
        let mut builder = Session::builder()?.with_intra_threads(threads).map_err(|e| anyhow!("{e}"))?;
        if device == Device::DirectMl {
            builder = builder
                .with_execution_providers([ort::ep::DirectML::default().build().error_on_failure()])
                .map_err(|e| anyhow!("DirectML kunde inte startas: {e}"))?
                // DirectML requires sequential execution without memory pattern optimisation.
                .with_parallel_execution(false)
                .map_err(|e| anyhow!("{e}"))?
                .with_memory_pattern(false)
                .map_err(|e| anyhow!("{e}"))?;
        }
        let session = builder.commit_from_file(path).map_err(|e| anyhow!("ordjusteringsmodellen kunde inte laddas: {e}"))?;
        Ok(Self { session, device })
    }

    /// Emissions for 16 kHz mono audio. `progress` gets 0..=100; returning false cancels.
    pub fn emissions(&mut self, audio: &[f32], mut progress: impl FnMut(i32) -> bool) -> Result<Emissions> {
        let frames = audio.len() / HOP;
        let mut lp: Vec<f32> = Vec::new();
        let mut vocab = 0;
        let mut window = vec![0f32; WINDOW];
        let mut core = 0;
        while core < audio.len() {
            if !progress((core as f64 / audio.len().max(1) as f64 * 100.0) as i32) {
                return Err(anyhow!("avbruten"));
            }
            // Fixed-size window around the core; shifted inward at the edges, zero-padded when the
            // whole file is shorter than one window. Normalised like the wav2vec2 processor.
            let lo = core.saturating_sub(PAD_SAMPLES).min(audio.len().saturating_sub(WINDOW));
            let hi = (lo + WINDOW).min(audio.len());
            let src = &audio[lo..hi];
            let mean = src.iter().map(|&x| x as f64).sum::<f64>() / src.len() as f64;
            let var = src.iter().map(|&x| (x as f64 - mean).powi(2)).sum::<f64>() / src.len() as f64;
            let scale = 1.0 / (var.sqrt() + 1e-7);
            window.fill(0.0);
            for (w, &x) in window.iter_mut().zip(src) {
                *w = ((x as f64 - mean) * scale) as f32;
            }
            let input = Tensor::from_array(([1usize, WINDOW], window.clone()))?;
            let outputs = self.session.run(ort::inputs! {"input_values" => input})?;
            let (shape, logits) = outputs["logits"].try_extract_tensor::<f32>()?;
            let (out_frames, v) = (shape[1] as usize, shape[2] as usize);
            if vocab == 0 {
                vocab = v;
                lp.reserve(frames * vocab);
            }
            let first = (core - lo) / HOP;
            let last = ((core + CORE_SAMPLES).min(audio.len()) - lo) / HOP;
            let wanted = (frames - lp.len() / vocab).min(last.saturating_sub(first));
            for f in first..(first + wanted).min(out_frames) {
                let row = &logits[f * v..(f + 1) * v];
                let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let log_sum = row.iter().map(|&x| (x - max).exp()).sum::<f32>().ln() + max;
                lp.extend(row.iter().map(|&x| x - log_sum));
            }
            core += CORE_SAMPLES;
        }
        progress(100);
        // Frame count can fall short by one at window joins; pad with the last row.
        while vocab > 0 && lp.len() / vocab < frames {
            let row = lp[lp.len() - vocab..].to_vec();
            lp.extend(row);
        }
        Ok(Emissions { lp, vocab })
    }
}
