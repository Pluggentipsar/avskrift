//! Small benchmark using Avskrift's pinned whisper-rs and decoding settings.
//! Does not include the app's cache, GPU fallback, diarization or user interface.
use std::{env, error::Error, fs, time::Instant};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        return Err("Usage: avskrift-whisper-probe MODEL WAV OUTPUT_JSON cpu|gpu".into());
    }
    let gpu = match args[4].as_str() {
        "cpu" => false,
        "gpu" if cfg!(feature = "vulkan") => true,
        _ => return Err("gpu requires a Vulkan build; mode must be cpu or gpu".into()),
    };
    let mut reader = hound::WavReader::open(&args[2])?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != 16000
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err("Input must be mono 16 kHz PCM16 WAV".into());
    }
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|v| v.map(|n| n as f32 / 32768.0))
        .collect::<Result<_, _>>()?;
    if samples.is_empty() {
        return Err("Empty audio".into());
    }
    let duration = samples.len() as f64 / 16000.0;
    let start = Instant::now();
    let mut context_params = WhisperContextParameters::default();
    context_params.use_gpu(gpu);
    let ctx = WhisperContext::new_with_params(&args[1], context_params)?;
    let mut state = ctx.create_state()?;
    let load_seconds = start.elapsed().as_secs_f64();
    let mut runs = Vec::new();
    for run in 0..3 {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("sv"));
        params.set_translate(false);
        params.set_no_context(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_token_timestamps(false);
        params.set_n_threads(8);
        let start = Instant::now();
        state.full(params, &samples)?;
        let mut bytes = Vec::new();
        for i in 0..state.full_n_segments()? {
            bytes.extend(state.full_get_segment_bytes(i)?);
        }
        let text = String::from_utf8_lossy(&bytes).trim().to_owned();
        let seconds = start.elapsed().as_secs_f64();
        eprintln!("run={} seconds={seconds:.3} audio={duration:.2}", run + 1);
        runs.push(serde_json::json!({"seconds": seconds, "text": text}));
    }
    let report = serde_json::json!({
        "engine": "whisper-rs 0.14.4 / whisper.cpp", "requested_gpu": gpu,
        "model": args[1], "audio": args[2], "audio_seconds": duration,
        "load_seconds": load_seconds, "threads": 8, "word_timestamps": false,
        "timing_scope": "Whisper full + segment extraction; excludes WAV loading",
        "runs": runs
    });
    fs::write(&args[3], serde_json::to_string_pretty(&report)?)?;
    Ok(())
}
