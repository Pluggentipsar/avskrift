//! Whisper word timestamps exactly as the app derives them (transcribe.rs: token timestamps,
//! space-led tokens start words). Baseline for the text-based video editing prototype.
use std::{env, error::Error, fs, time::Instant};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

fn is_continuation(b: u8) -> bool {
    b & 0xC0 == 0x80
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 5 {
        return Err("Usage: avskrift-textedit-probe MODEL WAV OUTPUT_JSON cpu|gpu".into());
    }
    let gpu = match args[4].as_str() {
        "cpu" => false,
        "gpu" if cfg!(feature = "vulkan") => true,
        _ => return Err("gpu requires a Vulkan build; mode must be cpu or gpu".into()),
    };
    let mut reader = hound::WavReader::open(&args[2])?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16000 || spec.bits_per_sample != 16 {
        return Err("Input must be mono 16 kHz PCM16 WAV".into());
    }
    let samples: Vec<f32> =
        reader.samples::<i16>().map(|v| v.map(|n| n as f32 / 32768.0)).collect::<Result<_, _>>()?;
    let mut context_params = WhisperContextParameters::default();
    context_params.use_gpu(gpu);
    let ctx = WhisperContext::new_with_params(&args[1], context_params)?;
    let mut state = ctx.create_state()?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("sv"));
    params.set_translate(false);
    params.set_no_context(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_token_timestamps(true);
    params.set_n_threads(8);
    let start = Instant::now();
    state.full(params, &samples)?;
    let seconds = start.elapsed().as_secs_f64();

    let mut segments = Vec::new();
    let mut words: Vec<(f64, f64, Vec<u8>)> = Vec::new();
    for i in 0..state.full_n_segments()? {
        let t0 = state.full_get_segment_t0(i)? as f64 / 100.0;
        let t1 = state.full_get_segment_t1(i)? as f64 / 100.0;
        let text = String::from_utf8_lossy(&state.full_get_segment_bytes(i)?).trim().to_owned();
        segments.push(serde_json::json!({"start": t0, "end": t1, "text": text}));
        // Mirrors transcribe.rs group_words: a whitespace-only token (" ") also ends a word.
        let mut boundary = true;
        for j in 0..state.full_n_tokens(i)? {
            let Ok(tok) = state.full_get_token_bytes(i, j) else { continue };
            if tok.starts_with(b"[_") {
                continue;
            }
            let Ok(data) = state.full_get_token_data(i, j) else { continue };
            if env::var_os("TEXTEDIT_TOKENS").is_some() {
                eprintln!("tok seg={i} t0={} t1={} {:?}", data.t0, data.t1, String::from_utf8_lossy(&tok));
            }
            let (s, e) = (data.t0 as f64 / 100.0, data.t1 as f64 / 100.0);
            let cont = tok.first().copied().is_some_and(is_continuation);
            if (boundary || tok.first().is_some_and(u8::is_ascii_whitespace)) && !cont {
                let piece: Vec<u8> = tok.iter().copied().skip_while(|b| b.is_ascii_whitespace()).collect();
                if piece.is_empty() {
                    boundary = true;
                    continue;
                }
                words.push((s, e, piece));
                boundary = false;
            } else if let Some(last) = words.last_mut() {
                last.2.extend_from_slice(&tok);
                last.1 = e;
            }
        }
    }
    let words: Vec<_> = words
        .into_iter()
        .map(|(s, e, b)| serde_json::json!({"start": s, "end": e, "text": String::from_utf8_lossy(&b)}))
        .collect();
    let report = serde_json::json!({
        "engine": "whisper-rs 0.14.4 / whisper.cpp", "gpu": gpu, "model": args[1],
        "audio_seconds": samples.len() as f64 / 16000.0, "seconds": seconds,
        "segments": segments, "words": words,
    });
    fs::write(&args[3], serde_json::to_string_pretty(&report)?)?;
    eprintln!("seconds={seconds:.2} words={}", report["words"].as_array().map_or(0, |w| w.len()));
    Ok(())
}
