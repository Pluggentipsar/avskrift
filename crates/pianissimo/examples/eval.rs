//! Opt-in accuracy/speed check: transcribe local 16 kHz mono WAV files (PCM16 or float) and
//! write the text per file as JSON, for scoring against references outside this crate.
//!
//! eval MODEL_DIR OUT.json [--max S] [--core S] [--context S] [--threads N] LIST.txt
//! LIST.txt holds one WAV path per line. Clips up to --max seconds run as one inference;
//! longer ones are split by `segments::transcribe_with`.
use anyhow::{ensure, Result};
use avskrift_pianissimo as p;
use std::{path::Path, time::Instant};

fn read(path: &str) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    ensure!(
        spec.channels == 1 && spec.sample_rate == 16000,
        "Expected mono 16 kHz: {path}"
    );
    Ok(match spec.sample_format {
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            ensure!(spec.bits_per_sample == 16, "Expected PCM16: {path}");
            reader
                .samples::<i16>()
                .map(|s| s.map(|s| s as f32 / 32768.0))
                .collect::<std::result::Result<_, _>>()?
        }
    })
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ensure!(args.len() >= 3, "MODEL_DIR OUT.json [options] LIST.txt");
    let (dir, out, list) = (Path::new(&args[0]), &args[1], &args[args.len() - 1]);
    let opt = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .map(|i| args[i + 1].parse::<f64>())
            .transpose()
    };
    let threads = opt("--threads")?.unwrap_or(8.0) as usize;
    let max = opt("--max")?.unwrap_or(120.0);
    let core = opt("--core")?.unwrap_or(110.0);
    let context = opt("--context")?.unwrap_or(5.0);
    let pool = ort::environment::GlobalThreadPoolOptions::default()
        .with_intra_threads(threads)?
        .with_inter_threads(1)?
        .with_spin_control(true)?;
    ensure!(
        ort::init().with_global_thread_pool(pool).commit(),
        "ORT already initialized"
    );
    let start = Instant::now();
    let (mut model, _) = p::model::Model::load(dir, threads, false, true)?;
    model.max_samples = (max * 16000.0) as usize;
    let load = start.elapsed().as_secs_f64();
    let mut results = Vec::new();
    let (mut audio_s, mut busy_s) = (0.0, 0.0);
    for path in std::fs::read_to_string(list)?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
    {
        let samples = read(path)?;
        let start = Instant::now();
        let text = if samples.len() <= model.max_samples {
            model
                .transcribe(&samples, || Ok(()))?
                .0
                .text
                .trim()
                .to_owned()
        } else {
            let segments = p::segments::transcribe_with(
                &mut model,
                &samples,
                (core * 16000.0) as usize,
                (context * 16000.0) as usize,
                || Ok(()),
                |_| {},
            )?;
            segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let seconds = start.elapsed().as_secs_f64();
        audio_s += samples.len() as f64 / 16000.0;
        busy_s += seconds;
        results.push(serde_json::json!({ "audio": path, "text": text, "seconds": seconds }));
        if results.len() % 50 == 0 {
            eprintln!("{} files, {:.1}x realtime", results.len(), audio_s / busy_s);
        }
    }
    eprintln!(
        "{} files, {audio_s:.0} s audio in {busy_s:.0} s ({:.1}x realtime), load {load:.1} s",
        results.len(),
        audio_s / busy_s
    );
    std::fs::write(
        out,
        serde_json::to_vec_pretty(&serde_json::json!({
            "model": dir, "threads": threads, "max": max, "core": core, "context": context,
            "load_seconds": load, "audio_seconds": audio_s, "busy_seconds": busy_s, "results": results
        }))?,
    )?;
    Ok(())
}
