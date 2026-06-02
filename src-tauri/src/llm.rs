//! Local Qwen GGUF inference via llama.cpp (`llama-cpp-2`), shared by the PII "Övrigt" layer
//! (`ai.rs`) and the meeting summary (`summarize.rs`).
//!
//! Replaces candle. candle's quantized inference was *equivalent* (candle and llama.cpp both
//! parroted the few-shot example at Q4); the real fix was Q8_0 weights + greedy decoding. We use
//! llama.cpp because it is meaningfully faster on CPU and shares the ggml backend with whisper.cpp.
//!
//! Built with `features = ["dynamic-link"]` so llama.cpp + its ggml live in their own DLLs and do
//! NOT collide at link time with whisper.cpp's statically-linked ggml (LNK2005). A GPU feature
//! (`vulkan`/`cuda`/`metal`) offloads layers to the GPU.

use std::num::NonZeroU32;
use std::path::Path;

use anyhow::{anyhow, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use once_cell::sync::Lazy;

const REPEAT_PENALTY: f32 = 1.15;
const REPEAT_LAST_N: i32 = 64;
const TOP_P: f32 = 0.9;
const SEED: u32 = 42;
/// Context window (Qwen2.5 supports 32k). Must be >= the longest prompt + output we generate; the
/// single-pass summary feeds up to ~6k tokens of transcript.
const N_CTX: u32 = 8192;

/// `LlamaBackend` is a zero-sized proof-of-init marker (its `&` args are unused); the real backend
/// is process-global C state guarded by an atomic, so it must be initialised exactly once. Wrapping
/// the marker lets us share that single init across Tauri's worker threads.
struct SyncBackend(LlamaBackend);
unsafe impl Sync for SyncBackend {}
static BACKEND: Lazy<SyncBackend> =
    Lazy::new(|| SyncBackend(LlamaBackend::init().expect("llama.cpp backend init")));

fn n_threads() -> i32 {
    std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4)
}

/// A loaded Qwen GGUF model. Each `generate` builds a fresh context (cheap next to the inference)
/// to avoid a self-referential model+context struct.
pub struct Qwen {
    model: LlamaModel,
}

impl Qwen {
    /// Load a GGUF model. llama.cpp uses the tokenizer embedded in the GGUF, so no separate
    /// tokenizer file is needed.
    pub fn load(gguf_path: &Path) -> Result<Self> {
        #[allow(unused_mut)]
        let mut params = LlamaModelParams::default();
        // Offload all layers to the GPU when built with a GPU backend feature; CPU otherwise.
        #[cfg(any(feature = "cuda", feature = "metal", feature = "vulkan"))]
        {
            params = params.with_n_gpu_layers(999);
        }
        let model = LlamaModel::load_from_file(&BACKEND.0, gguf_path, &params)
            .map_err(|e| anyhow!("kunde inte ladda modell {}: {e}", gguf_path.display()))?;
        Ok(Self { model })
    }

    /// Generate up to `max_new` tokens. `temperature <= 0.0` => greedy (argmax) — what these
    /// extraction/summary tasks want (faithful, reproducible; low-temp sampling made the quantized
    /// model parrot the few-shot example). A repeat penalty keeps greedy from looping.
    pub fn generate(&self, prompt: &str, max_new: usize, temperature: f32) -> Result<String> {
        let threads = n_threads();
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            // n_batch must be >= the prefill length (one llama_decode call) or llama.cpp asserts
            // n_tokens_all <= n_batch; match it to the context window.
            .with_n_batch(N_CTX)
            .with_n_threads(threads)
            .with_n_threads_batch(threads);
        let mut ctx = self
            .model
            .new_context(&BACKEND.0, ctx_params)
            .map_err(|e| anyhow!("kunde inte skapa kontext: {e}"))?;

        let tokens = self
            .model
            .str_to_token(prompt, AddBos::Never)
            .map_err(|e| anyhow!("tokenisering misslyckades: {e}"))?;
        if tokens.is_empty() {
            return Ok(String::new());
        }

        // Prefill the whole prompt in one batch; only the last token needs logits.
        let mut batch = LlamaBatch::new(N_CTX as usize, 1);
        let last = tokens.len() - 1;
        for (i, t) in tokens.iter().enumerate() {
            batch.add(*t, i as i32, &[0], i == last).map_err(|e| anyhow!("batch-fel: {e}"))?;
        }
        ctx.decode(&mut batch).map_err(|e| anyhow!("prefill-decode misslyckades: {e}"))?;

        let mut sampler = if temperature <= 0.0 {
            LlamaSampler::chain_simple([
                LlamaSampler::penalties(REPEAT_LAST_N, REPEAT_PENALTY, 0.0, 0.0),
                LlamaSampler::greedy(),
            ])
        } else {
            LlamaSampler::chain_simple([
                LlamaSampler::penalties(REPEAT_LAST_N, REPEAT_PENALTY, 0.0, 0.0),
                LlamaSampler::top_p(TOP_P, 1),
                LlamaSampler::temp(temperature),
                LlamaSampler::dist(SEED),
            ])
        };

        let mut n_cur = batch.n_tokens();
        let mut bytes: Vec<u8> = Vec::new();
        for _ in 0..max_new {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);
            if self.model.is_eog_token(token) {
                break;
            }
            // token_to_bytes retries past the small default buffer (tokens_to_str does not).
            let piece = self
                .model
                .token_to_bytes(token, Special::Plaintext)
                .map_err(|e| anyhow!("avkodning misslyckades: {e}"))?;
            bytes.extend_from_slice(&piece);

            batch.clear();
            batch.add(token, n_cur, &[0], true).map_err(|e| anyhow!("batch-fel: {e}"))?;
            n_cur += 1;
            ctx.decode(&mut batch).map_err(|e| anyhow!("decode misslyckades: {e}"))?;
        }
        // Decode the full byte run at once so multi-byte UTF-8 spanning tokens reassembles.
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}
