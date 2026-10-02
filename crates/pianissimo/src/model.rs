//! CPU-only engine for Klang's pinned Pianissimo ONNX export (FastConformer encoder, TDT decoder).
use anyhow::{ensure, Result};
use ort::{session::Session, value::Tensor};
use regex::{Captures, Regex};
use serde::Serialize;
use std::{path::Path, time::Instant};

const BLANK: usize = 8192;
pub struct Model {
    encoder: crate::encoder::Encoder,
    decoder: Session,
    vocab: Vec<String>,
    /// Longest single inference. The segmenter keeps every pass within this.
    pub max_samples: usize,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Transcript {
    pub text: String,
    pub tokens: Vec<String>,
    pub token_ids: Vec<usize>,
    /// Token starts, not word boundaries or word ends.
    pub timestamps: Vec<f64>,
}
#[derive(Serialize)]
pub struct Timings {
    pub features: f64,
    pub encoder: f64,
    pub decoder: f64,
}

pub(crate) fn session(
    path: &Path,
    threads: usize,
    flush: bool,
    prepack: bool,
    spin: bool,
    arena: bool,
    optimized: bool,
) -> Result<Session> {
    let mut builder = Session::builder()?
        .with_execution_providers([ort::ep::CPU::default().with_arena_allocator(arena).build()])
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_intra_threads(threads)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_inter_threads(1)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_precise_qmm()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_prepacking(prepack)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_intra_op_spinning(spin)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_inter_op_spinning(spin)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    if flush {
        builder = builder
            .with_flush_to_zero()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    if optimized {
        builder = builder
            .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Disable)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    Ok(builder.commit_from_file(path)?)
}

impl Model {
    pub fn load(
        dir: &Path,
        threads: usize,
        flush: bool,
        prepack: bool,
    ) -> Result<(Self, [f64; 2])> {
        ensure!(threads > 0, "threads must be positive");
        let cfg: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("config.json"))?)?;
        ensure!(
            cfg["model_type"] == "nemo-conformer-tdt"
                && cfg["features_size"] == 128
                && cfg["subsampling_factor"] == 8
                && cfg["max_tokens_per_step"] == 10,
            "Unsupported model configuration"
        );
        let mut vocab = Vec::new();
        for line in std::fs::read_to_string(dir.join("vocab.txt"))?.lines() {
            let (token, id) = line
                .rsplit_once(' ')
                .ok_or_else(|| anyhow::anyhow!("Invalid vocab line"))?;
            ensure!(
                id.parse::<usize>()? == vocab.len(),
                "Vocab IDs must be contiguous"
            );
            vocab.push(token.replace('▁', " "));
        }
        ensure!(vocab.len() == BLANK + 1, "Unsupported vocabulary size");
        let start = Instant::now();
        // Klang's graph is small enough to load whole in a few seconds; splitting it into
        // stages (as the earlier community export needed) made inference slower and larger.
        let encoder = crate::encoder::Encoder::new(session(
            &dir.join("encoder-model.int8.onnx"),
            threads,
            flush,
            prepack,
            true,
            true,
            false,
        )?);
        let encoder_seconds = start.elapsed().as_secs_f64();
        eprintln!("Encoder loaded in {encoder_seconds:.3}s");
        let start = Instant::now();
        let decoder = session(
            &dir.join("decoder_joint-model.int8.onnx"),
            threads,
            flush,
            prepack,
            true,
            true,
            false,
        )?;
        let decoder_seconds = start.elapsed().as_secs_f64();
        Ok((
            Self {
                encoder,
                decoder,
                vocab,
                max_samples: crate::segments::MAX_PASS,
            },
            [encoder_seconds, decoder_seconds],
        ))
    }

    // Caller supplies a cancellation checkpoint; state is local to each recording.
    // Synchronous ORT runs cannot be interrupted by this prototype.
    pub fn transcribe(
        &mut self,
        audio: &[f32],
        check: impl Fn() -> Result<()>,
    ) -> Result<(Transcript, Timings)> {
        check()?;
        ensure!(
            audio.len() <= self.max_samples,
            "Pianissimo behöver dela långt ljud i kortare avsnitt"
        );
        let start = Instant::now();
        let (features, frames, valid) = crate::features::extract(audio)?;
        let features_seconds = start.elapsed().as_secs_f64();
        check()?;
        let start = Instant::now();
        let output = self.encoder.run(features, frames, valid, &check)?;
        let (shape, encoded) = output["outputs"].try_extract_tensor::<f32>()?;
        let (_, lengths) = output["encoded_lengths"].try_extract_tensor::<i64>()?;
        ensure!(
            shape.len() == 3 && shape[0] == 1 && shape[1] == 1024 && shape[2] > 0,
            "Invalid encoder shape"
        );
        let stride = shape[2] as usize;
        ensure!(
            lengths.len() == 1 && lengths[0] > 0 && lengths[0] as usize <= stride,
            "Invalid encoder length"
        );
        ensure!(
            encoded.iter().all(|x| x.is_finite()),
            "Non-finite encoder output"
        );
        let encoder_seconds = start.elapsed().as_secs_f64();
        let start = Instant::now();
        let mut state1 = vec![0.0_f32; 1280];
        let mut state2 = state1.clone();
        let mut token_ids = Vec::new();
        let mut timestamps = Vec::new();
        let (mut t, mut emitted) = (0, 0);
        while t < lengths[0] as usize {
            check()?;
            let frame: Vec<_> = (0..1024).map(|c| encoded[c * stride + t]).collect();
            let out = self.decoder.run(ort::inputs! {
                "encoder_outputs" => Tensor::from_array(([1,1024,1], frame))?,
                "targets" => Tensor::from_array(([1,1], vec![*token_ids.last().unwrap_or(&BLANK) as i32]))?,
                "target_length" => Tensor::from_array(([1], vec![1_i32]))?,
                "input_states_1" => Tensor::from_array(([2,1,640], state1.clone()))?,
                "input_states_2" => Tensor::from_array(([2,1,640], state2.clone()))?,
            })?;
            let (_, logits) = out["outputs"].try_extract_tensor::<f32>()?;
            ensure!(
                logits.len() == 8198 && logits.iter().all(|x| x.is_finite()),
                "Invalid decoder logits"
            );
            let token = argmax(&logits[..=BLANK]);
            let step = argmax(&logits[BLANK + 1..]);
            if token != BLANK {
                let (_, h) = out["output_states_1"].try_extract_tensor::<f32>()?;
                let (_, c) = out["output_states_2"].try_extract_tensor::<f32>()?;
                ensure!(
                    h.len() == 1280 && c.len() == 1280 && h.iter().chain(c).all(|x| x.is_finite()),
                    "Invalid decoder state"
                );
                state1.copy_from_slice(h);
                state2.copy_from_slice(c);
                token_ids.push(token);
                timestamps.push(t as f64 * 0.08);
                emitted += 1;
            }
            if step > 0 {
                t += step;
                emitted = 0;
            } else if token == BLANK || emitted == 10 {
                t += 1;
                emitted = 0;
            }
        }
        let tokens: Vec<_> = token_ids.iter().map(|&id| self.vocab[id].clone()).collect();
        let text = clean_text(&tokens.concat());
        let decoder_seconds = start.elapsed().as_secs_f64();
        Ok((
            Transcript {
                text,
                tokens,
                token_ids,
                timestamps,
            },
            Timings {
                features: features_seconds,
                encoder: encoder_seconds,
                decoder: decoder_seconds,
            },
        ))
    }
}
fn argmax(values: &[f32]) -> usize {
    // First maximum, matching NumPy (Iterator::max_by would return the last tie).
    (1..values.len()).fold(0, |best, i| if values[i] > values[best] { i } else { best })
}
pub fn clean_text(text: &str) -> String {
    Regex::new(r"\A\s|\s\B|(\s)\b")
        .unwrap()
        .replace_all(
            text,
            |c: &Captures| {
                if c.get(1).is_some() {
                    " "
                } else {
                    ""
                }
            },
        )
        .into_owned()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_maximum_wins() {
        assert_eq!(argmax(&[1.0, 2.0, 2.0]), 1);
    }
    #[test]
    fn swedish_punctuation() {
        assert_eq!(clean_text(" Åsa är här ."), "Åsa är här.");
    }
}
