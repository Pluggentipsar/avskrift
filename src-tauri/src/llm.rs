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

use crate::memory::{self, Cache, DeviceFailure};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::TokenToStringError;
use once_cell::sync::Lazy;

const REPEAT_PENALTY: f32 = 1.15;
const REPEAT_LAST_N: i32 = 64;
const TOP_P: f32 = 0.9;
const SEED: u32 = 42;
/// Application ceiling; each request also respects the loaded model's trained context length.
const N_CTX: u32 = 8192;
const BATCH: usize = 512;

#[cfg(test)]
fn context_size(prompt_tokens: usize, max_new: usize) -> Result<u32> {
    context_size_with_limit(prompt_tokens, max_new, N_CTX as usize)
}

fn context_size_with_limit(prompt_tokens: usize, max_new: usize, limit: usize) -> Result<u32> {
    let needed = prompt_tokens.checked_add(max_new).ok_or_else(|| anyhow!("texten är för lång"))?;
    if needed > limit {
        return Err(anyhow!("Texten och önskad svarslängd överskrider modellens arbetsfönster ({limit} token). Dela upp underlaget i mindre delar."));
    }
    Ok((needed.max(512).div_ceil(256) * 256).min(limit) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_preserves_the_whole_prompt_and_output_budget() {
        assert_eq!(context_size(100, 512).unwrap(), 768);
        assert_eq!(context_size(7168, 1024).unwrap(), 8192);
        assert!(context_size(7169, 1024).is_err());
        assert!(context_size(usize::MAX, 1).is_err());
        assert_eq!(context_size_with_limit(1800, 200, 2000).unwrap(), 2000);
        assert!(context_size_with_limit(1801, 200, 2000).is_err());
        for prompt in 1..8192 {
            let size = context_size(prompt, 1).unwrap() as usize;
            assert!(size >= prompt + 1 && size <= 8192 && size % 256 == 0);
        }
    }

    /// Opt-in integration test against an existing local GGUF, including multi-batch prefill.
    #[test]
    #[ignore]
    fn real_model_handles_batched_prompts_and_independent_requests() {
        let path = std::env::var("AVSKRIFT_BENCH_LLM").expect("AVSKRIFT_BENCH_LLM");
        let model = Qwen::load(Path::new(&path)).unwrap();
        let body = "Mötet sker på torsdag. Vi ska måla bibliotekets väggar blå. ".repeat(75);
        let prompt = format!("<|im_start|>system\nSvara kort på svenska.<|im_end|>\n<|im_start|>user\n{body}\nVilken färg ska väggarna få?<|im_end|>\n<|im_start|>assistant\n");
        let count = crate::text_budget::TextEngine::token_count(&model, &prompt).unwrap();
        assert!(count > BATCH);
        let start = std::time::Instant::now();
        let output = model.generate(&prompt, 32, 0.0).unwrap();
        assert!(output.to_lowercase().contains("blå"), "syntetisk faktafråga saknar rätt färg");
        println!(
            "BENCH llm prompt_tokens={count} context={} elapsed_ms={:.2} characters={}",
            context_size(count, 32).unwrap(),
            start.elapsed().as_secs_f64() * 1000.0,
            output.chars().count()
        );
        let other = "<|im_start|>user\nSkriv endast siffran 7.<|im_end|>\n<|im_start|>assistant\n";
        assert!(model.generate(other, 8, 0.0).unwrap().contains('7'));
        assert_eq!(model.generate(&prompt, 32, 0.0).unwrap(), output);
    }
}

/// `LlamaBackend` is a zero-sized proof-of-init marker (its `&` args are unused); the real backend
/// is process-global C state guarded by an atomic, so it must be initialised exactly once. Wrapping
/// the marker lets us share that single init across Tauri's worker threads.
struct SyncBackend(LlamaBackend);
unsafe impl Sync for SyncBackend {}
static BACKEND: Lazy<SyncBackend> = Lazy::new(|| SyncBackend(LlamaBackend::init().expect("llama.cpp backend init")));

fn n_threads() -> i32 {
    num_cpus::get_physical().clamp(1, 8) as i32
}

/// Lightweight model reference. All consumers (PII, drafts, summary) share one native allocation.
pub struct Qwen {
    path: PathBuf,
    context_limit: usize,
}
struct LoadedQwen {
    devices: Vec<usize>,
    model: LlamaModel,
    gpu: bool,
    path: PathBuf,
    mode: String,
}
static MODELS: Lazy<Mutex<Cache<LoadedQwen>>> = Lazy::new(|| Mutex::new(Cache::new()));
#[cfg(test)]
static LOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(crate) fn init_backend() {
    Lazy::force(&BACKEND);
}
pub(crate) fn release_cached() {
    if let Ok(mut c) = MODELS.try_lock() {
        c.clear();
    }
}
pub(crate) fn sweep(now: std::time::Instant, pressure: bool) {
    if let Ok(mut c) = MODELS.try_lock() {
        c.sweep(now, pressure);
    }
}
pub(crate) fn cache_status() -> Option<String> {
    MODELS.try_lock().ok()?.value.as_ref().map(|m| m.mode.clone())
}

impl crate::text_budget::TextEngine for Qwen {
    fn token_count(&self, text: &str) -> Result<usize> {
        self.with_model(|m| {
            Ok(m.model.str_to_token(text, AddBos::Never).map_err(|e| anyhow!("tokenisering misslyckades: {e}"))?.len())
        })
    }
    fn context_limit(&self) -> usize {
        self.context_limit
    }
    fn complete(&self, prompt: &str, output: usize) -> Result<String> {
        self.generate_complete(prompt, output)
    }
}
impl Qwen {
    pub fn load(path: &Path) -> Result<Self> {
        let path =
            std::fs::canonicalize(path).map_err(|e| anyhow!("kunde inte öppna modell {}: {e}", path.display()))?;
        let mut handle = Self { path, context_limit: N_CTX as usize };
        handle.context_limit = handle.with_model(|m| Ok(N_CTX.min(m.model.n_ctx_train()) as usize))?;
        Ok(handle)
    }
    fn with_model<T>(&self, run: impl FnOnce(&LoadedQwen) -> Result<T>) -> Result<T> {
        let _work = memory::enter()?;
        let mut cache = MODELS.lock().map_err(|_| anyhow!("Textmodellens cache behöver startas om."))?;
        Self::ensure(&mut cache, &self.path)?;
        crate::work::check()?;
        let result = run(cache.value.as_ref().unwrap());
        crate::work::check()?;
        cache.touch();
        result
    }
    fn ensure(cache: &mut Cache<LoadedQwen>, path: &Path) -> Result<()> {
        if cache.value.as_ref().is_none_or(|m| m.path != path) {
            cache.clear(); // Drop before loading replacement, including on load failure.
            cache.value = Some(LoadedQwen::load(path, None)?);
        }
        Ok(())
    }
    fn generate_inner(
        &self,
        prompt: &str,
        max_new: usize,
        temperature: f32,
        grammar: Option<&str>,
        require_complete: bool,
    ) -> Result<String> {
        let _work = memory::enter()?;
        let mut cache = MODELS.lock().map_err(|_| anyhow!("Textmodellens cache behöver startas om."))?;
        Self::ensure(&mut cache, &self.path)?;
        crate::work::check()?;
        if cache.value.as_ref().unwrap().gpu
            && memory::sample().devices_under_pressure(&cache.value.as_ref().unwrap().devices)
        {
            crate::transcribe::release_cached();
            if memory::sample().devices_under_pressure(&cache.value.as_ref().unwrap().devices) {
                cache.clear();
                cache.value = Some(LoadedQwen::load(&self.path, Some("CPU – lite ledigt grafikminne"))?);
            }
        }
        let run = |m: &LoadedQwen| m.generate_inner(prompt, max_new, temperature, grammar, require_complete);
        let result = run(cache.value.as_ref().unwrap());
        let result = match result {
            Err(e) if memory::can_retry(cache.value.as_ref().unwrap().gpu, &e) => {
                crate::work::check()?;
                cache.clear(); // No partial text escapes and GPU allocation is gone before CPU load.
                crate::transcribe::release_cached();
                cache.value =
                    Some(LoadedQwen::load(&self.path, Some("CPU – reservväg efter GPU-fel")).map_err(|cpu| {
                        anyhow!("GPU-körningen misslyckades ({e}). CPU-reservvägen kunde inte starta: {cpu}")
                    })?);
                run(cache.value.as_ref().unwrap())
                    .map_err(|cpu| anyhow!("GPU-körningen misslyckades ({e}). Även CPU-försöket misslyckades: {cpu}"))
            }
            other => other,
        };
        crate::work::check()?;
        cache.touch();
        result
    }
    /// Generate up to `max_new` tokens. `temperature <= 0.0` => greedy (argmax) — what these
    /// extraction/summary tasks want (faithful, reproducible; low-temp sampling made the quantized
    /// model parrot the few-shot example). A repeat penalty keeps greedy from looping.
    pub fn generate(&self, prompt: &str, max_new: usize, temperature: f32) -> Result<String> {
        self.generate_inner(prompt, max_new, temperature, None, false)
    }

    pub fn generate_structured(&self, prompt: &str, max_new: usize, grammar: &str) -> Result<String> {
        self.generate_inner(prompt, max_new, 0.0, Some(grammar), true)
    }

    pub fn generate_complete(&self, prompt: &str, max_new: usize) -> Result<String> {
        self.generate_inner(prompt, max_new, 0.0, None, true)
    }
}

impl LoadedQwen {
    fn generate_inner(
        &self,
        prompt: &str,
        max_new: usize,
        temperature: f32,
        grammar: Option<&str>,
        require_complete: bool,
    ) -> Result<String> {
        crate::work::check()?;
        if max_new == 0 {
            return Ok(String::new());
        }
        let tokens =
            self.model.str_to_token(prompt, AddBos::Never).map_err(|e| anyhow!("tokenisering misslyckades: {e}"))?;
        if tokens.is_empty() {
            return Ok(String::new());
        }
        // Size the KV cache to this request, retaining the full requested output budget.
        let n_ctx = context_size_with_limit(tokens.len(), max_new, N_CTX.min(self.model.n_ctx_train()) as usize)?;
        let threads = n_threads();
        #[cfg(test)]
        if memory::take_fault(memory::Fault::TextContext) {
            return Err(anyhow!(DeviceFailure("syntetiskt kontextallokeringsfel".into())));
        }
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(n_ctx))
            .with_n_batch(BATCH as u32)
            .with_n_ubatch(BATCH as u32)
            .with_offload_kqv(self.gpu)
            .with_op_offload(self.gpu)
            .with_n_threads(threads)
            .with_n_threads_batch(threads);
        let mut ctx = self
            .model
            .new_context(&BACKEND.0, ctx_params)
            .map_err(|e| anyhow!(DeviceFailure(format!("kunde inte skapa kontext: {e}"))))?;

        // Bound prefill allocations without dropping any prompt tokens or resetting positions.
        let mut batch = LlamaBatch::new(BATCH, 1);
        let last = tokens.len() - 1;
        for (chunk_index, chunk) in tokens.chunks(BATCH).enumerate() {
            batch.clear();
            for (offset, t) in chunk.iter().enumerate() {
                let i = chunk_index * BATCH + offset;
                batch.add(*t, i as i32, &[0], i == last).map_err(|e| anyhow!("batch-fel: {e}"))?;
            }
            crate::work::check()?;
            let decoded = ctx.decode(&mut batch);
            crate::work::check()?;
            decoded.map_err(native_decode_error)?;
        }

        let mut sampler = if let Some(grammar) = grammar {
            // Exact quotations deliberately repeat source text: no repetition penalty here.
            LlamaSampler::chain_simple([
                LlamaSampler::grammar(&self.model, grammar, "root")
                    .map_err(|e| anyhow!("ogiltig svarsstruktur: {e}"))?,
                LlamaSampler::greedy(),
            ])
        } else if temperature <= 0.0 {
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

        let mut n_cur = tokens.len() as i32;
        let mut bytes: Vec<u8> = Vec::new();
        let mut complete = false;
        for generated in 0..max_new {
            crate::work::check()?;
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            // sample() already accepts the token in this llama.cpp version. Accepting twice
            // corrupts grammar state and also counts repeated tokens twice in ordinary generation.
            if self.model.is_eog_token(token) {
                complete = true;
                break;
            }
            // Retry past the small default 8-byte buffer for tokens that decode to more bytes
            // (multi-byte UTF-8, e.g. åäö) — mirrors the now-deprecated `token_to_bytes`.
            let piece = match self.model.token_to_piece_bytes(token, 8, false, None) {
                Err(TokenToStringError::InsufficientBufferSpace(i)) => self.model.token_to_piece_bytes(
                    token,
                    (-i).try_into().expect("error buffer size is positive"),
                    false,
                    None,
                ),
                x => x,
            }
            .map_err(|e| anyhow!("avkodning misslyckades: {e}"))?;
            bytes.extend_from_slice(&piece);

            if generated + 1 == max_new {
                break;
            }

            batch.clear();
            batch.add(token, n_cur, &[0], true).map_err(|e| anyhow!("batch-fel: {e}"))?;
            n_cur += 1;
            crate::work::check()?;
            let decoded = ctx.decode(&mut batch);
            crate::work::check()?;
            decoded.map_err(native_decode_error)?;
        }
        // Decode the full byte run at once so multi-byte UTF-8 spanning tokens reassembles.
        if require_complete && !complete {
            return Err(anyhow!("Modellens svar blev för långt och avbröts. Förkorta underlaget eller mallen och försök igen. Tidigare text finns kvar."));
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

fn native_decode_error(e: llama_cpp_2::DecodeError) -> anyhow::Error {
    match e {
        llama_cpp_2::DecodeError::Unknown(_) => anyhow!(DeviceFailure(format!("modellberäkningen misslyckades: {e}"))),
        _ => anyhow!("modellberäkningen misslyckades: {e}"),
    }
}
impl LoadedQwen {
    // Called with WORK and cache held. Fit accounts for weights, KV and compute at the full
    // app context ceiling; no silently reduced context and no change to the user's model.
    fn load(path: &Path, cpu_reason: Option<&str>) -> Result<Self> {
        init_backend();
        let bytes = std::fs::metadata(path)?.len();
        let mut info = memory::sample();
        if info.ram_pressure() || !info.speech_gpu_fits(bytes.saturating_add(1024 * memory::MIB)) {
            crate::transcribe::release_cached();
            info = memory::sample();
        }
        let available =
            cfg!(any(feature = "vulkan", feature = "cuda", feature = "metal")) && BACKEND.0.supports_gpu_offload();
        // Native default selection prefers discrete GPUs and excludes iGPUs when discrete ones
        // exist. Never pass ggml pointers from Rust: Whisper's static ggml has conflicting symbols.
        // All default discrete devices must have a usable memory report before native fitting.
        let devices: Vec<_> = info.text_devices().into_iter().take(llama_cpp_2::max_devices().min(16)).collect();
        let use_gpu = cpu_reason.is_none()
            && available
            && !devices.is_empty()
            && info.gpus.iter().filter(|g| !g.integrated).all(|g| g.free > memory::GPU_MARGIN);
        let mut mode = cpu_reason.unwrap_or("CPU – automatisk minnesanpassning").to_string();
        let mut params = Box::pin(LlamaModelParams::default().with_n_gpu_layers(0));
        let mut gpu = false;
        if use_gpu {
            let mut fitted = Box::pin(LlamaModelParams::default());
            let mut ctx = LlamaContextParams::default()
                .with_n_ctx(NonZeroU32::new(N_CTX))
                .with_n_batch(BATCH as u32)
                .with_n_ubatch(BATCH as u32);
            let name = std::ffi::CString::new(path.to_str().ok_or_else(|| anyhow!("ogiltig modellsökväg"))?)?;
            let mut margins = vec![memory::GPU_MARGIN as usize; llama_cpp_2::max_devices()];
            if fitted.as_mut().fit_params(&name, &mut ctx, &mut margins, N_CTX, 2).is_ok() {
                gpu = fitted.n_gpu_layers() != 0;
                mode = if !gpu {
                    "CPU – modellen ryms inte på GPU med minnesmarginal".into()
                } else if fitted.n_gpu_layers() < 0 {
                    "GPU – modellen ryms med minnesmarginal".into()
                } else {
                    format!("GPU/CPU – {} lager på GPU", fitted.n_gpu_layers())
                };
                params = fitted;
            }
        }
        if (!gpu || params.n_gpu_layers() >= 0) && !info.cpu_fits(bytes.saturating_add(512 * memory::MIB)) {
            return Err(anyhow!("För lite ledigt arbetsminne för textmodellen. Stäng andra program eller välj en mindre modell under Modeller på datorn."));
        }
        let model = match LlamaModel::load_from_file(&BACKEND.0, path, &params) {
            Ok(model) => model,
            Err(e) if gpu => {
                crate::work::check()?;
                // Failed native model loads have released their partial allocation before return.
                return Self::load(path, Some("CPU – reservväg efter GPU-fel"))
                    .map_err(|cpu| anyhow!("GPU-modellen kunde inte laddas ({e}). CPU: {cpu}"));
            }
            Err(e) => return Err(anyhow!("kunde inte ladda modell {}: {e}", path.display())),
        };
        eprintln!("AVskrift textmodell: {mode}");
        #[cfg(test)]
        LOADS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Self { model, gpu, path: path.to_path_buf(), mode, devices: if gpu { devices } else { vec![] } })
    }
}

#[cfg(test)]
mod memory_native_tests {
    use super::*;
    #[test]
    #[ignore]
    fn cancellation_native_text_then_fresh_request() {
        let path = std::env::var("AVSKRIFT_DRAFT_MODEL").unwrap();
        let q = Qwen::load(Path::new(&path)).unwrap();
        let loads = LOADS.load(std::sync::atomic::Ordering::SeqCst);
        let id = crate::work::begin().unwrap(); let cancel_id = id.clone();
        let cancel = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            assert!(crate::work::cancel(&cancel_id));
        });
        let start = std::time::Instant::now();
        let result = crate::work::run(Some(id), || q.generate_complete("<|im_start|>user\nSkriv talen från 1 till 1000. Skriv alla talen utan att avbryta.<|im_end|>\n<|im_start|>assistant\n",4096));
        cancel.join().unwrap(); assert!(result.unwrap_err().is::<crate::work::Cancelled>());
        assert!(start.elapsed().as_secs() < 20);
        assert_eq!(LOADS.load(std::sync::atomic::Ordering::SeqCst),loads,"cancellation must not trigger CPU reload");
        assert!(q.generate_complete("<|im_start|>user\nSkriv endast siffran 7.<|im_end|>\n<|im_start|>assistant\n",32).unwrap().contains('7'));
        println!("CANCEL text: stopped at decode boundary, no CPU retry, fresh answer passed");
    }
    #[test]
    #[ignore]
    fn shared_text_cache_and_cpu_recovery_with_real_model() {
        let path = std::env::var("AVSKRIFT_DRAFT_MODEL").expect("existing GGUF path");
        let q = Qwen::load(Path::new(&path)).unwrap();
        if cfg!(windows) {
            assert!(
                q.with_model(|_| Ok(memory::sample().ram_free.is_some())).unwrap(),
                "RAM report must come from the loaded text engine DLLs"
            );
        }
        let loads = LOADS.load(std::sync::atomic::Ordering::SeqCst);
        let first = q.with_model(|m| Ok(std::ptr::addr_of!(m.model) as usize)).unwrap();
        let second = Qwen::load(Path::new(&path)).unwrap();
        assert_eq!(loads, LOADS.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(first, second.with_model(|m| Ok(std::ptr::addr_of!(m.model) as usize)).unwrap());
        let gpu = q.with_model(|m| Ok(m.gpu)).unwrap();
        println!("MEMORY text shared=true gpu={gpu} limit={}", q.context_limit);
        let prompt = "<|im_start|>user\nSkriv endast siffran 7.<|im_end|>\n<|im_start|>assistant\n";
        if gpu {
            memory::inject(memory::Fault::TextContext);
            let answer = q.generate_complete(prompt, 32).unwrap();
            assert!(answer.contains('7'));
            assert!(!q.with_model(|m| Ok(m.gpu)).unwrap());
            assert!(cache_status().unwrap().contains("reservväg"));
            assert_eq!(q.generate_complete(prompt, 32).unwrap(), answer);
            println!("MEMORY text injected-context-failure -> real CPU response passed");
        } else {
            assert!(q.generate_complete(prompt, 32).unwrap().contains('7'));
        }
        {
            let _work = memory::enter().unwrap();
            // Maintenance cannot enter while a request owns native memory.
            assert!(memory::WORK.try_lock().is_err());
            sweep(std::time::Instant::now() + memory::IDLE + std::time::Duration::from_secs(1), false);
            assert!(cache_status().is_none());
        }
        // Existing lightweight handles remain valid after eviction (no dangling model/context).
        assert!(second.generate_complete(prompt, 32).unwrap().contains('7'));
        println!("MEMORY text expiry and reload passed");
    }

    #[test]
    #[ignore]
    fn native_fit_with_a_small_gpu_budget_preserves_context() {
        if !cfg!(any(feature = "vulkan", feature = "cuda", feature = "metal")) {
            return;
        }
        let path = std::env::var("AVSKRIFT_DRAFT_MODEL").expect("existing GGUF path");
        let _work = memory::enter().unwrap();
        release_cached();
        crate::transcribe::release_cached();
        let info = memory::sample();
        assert_eq!(info.gpus.len(), 1, "this opt-in budget fixture requires one discrete GPU");
        assert!(!info.gpus[0].integrated);
        // Artificially reserve the rest of the GPU, without allocating it or exhausting memory.
        let cap = 2 * 1024 * memory::MIB;
        assert!(info.gpus[0].free > cap);
        let mut margins = vec![(info.gpus[0].free - cap) as usize; llama_cpp_2::max_devices()];
        let mut params = Box::pin(LlamaModelParams::default());
        let mut ctx = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_batch(BATCH as u32)
            .with_n_ubatch(BATCH as u32);
        let fitted = params
            .as_mut()
            .fit_params(&std::ffi::CString::new(path.clone()).unwrap(), &mut ctx, &mut margins, N_CTX, 2)
            .unwrap();
        assert_eq!(fitted.n_ctx, N_CTX);
        let layers = params.n_gpu_layers();
        let model = LlamaModel::load_from_file(&BACKEND.0, Path::new(&path), &params).unwrap();
        assert!(layers > 0 && layers < (model.n_layer() + 1) as i32, "expected reduced offload, got {layers}");
        let loaded =
            LoadedQwen { model, gpu: layers > 0, path: path.into(), mode: "test".into(), devices: info.text_devices() };
        let prompt = "<|im_start|>user\nSkriv endast siffran 7.<|im_end|>\n<|im_start|>assistant\n";
        assert!(loaded.generate_inner(prompt, 32, 0.0, None, true).unwrap().contains('7'));
        println!(
            "MEMORY constrained GPU budget=2048MiB layers={layers} context={} real generation passed",
            fitted.n_ctx
        );
    }
}
