#[path = "../../../crates/pianissimo/src/encoder.rs"]
mod encoder;
#[path = "../../../crates/pianissimo/src/features.rs"]
mod features;
#[path = "../../../crates/pianissimo/src/model.rs"]
mod model;
use anyhow::{ensure, Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Instant,
};

fn audio(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    ensure!(
        spec.channels == 1
            && spec.sample_rate == 16000
            && spec.bits_per_sample == 16
            && spec.sample_format == hound::SampleFormat::Int,
        "Expected mono 16 kHz PCM16 WAV"
    );
    ensure!(
        (512..=120 * 16000).contains(&reader.duration()),
        "Expected 0.032–120 seconds of audio"
    );
    Ok(reader
        .samples::<i16>()
        .map(|x| x.map(|n| n as f32 / 32768.0))
        .collect::<Result<Vec<_>, _>>()?)
}
fn hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("features") {
        ensure!(args.len() == 4, "features WAV OUTPUT.f32");
        let (values, frames, valid) = features::extract(&audio(Path::new(&args[2]))?)?;
        let mut file = fs::File::create(&args[3])?;
        for x in values {
            file.write_all(&x.to_le_bytes())?;
        }
        println!("frames={frames} valid={valid}");
        return Ok(());
    }
    ensure!(args.len() >= 4, "MODEL_DIR OUTPUT.json WAV...; env: PIANISSIMO_THREADS=8, PIANISSIMO_FLUSH=1, PIANISSIMO_NO_PREPACK=1");
    let dir = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    let threads = std::env::var("PIANISSIMO_THREADS")
        .unwrap_or("8".into())
        .parse::<usize>()?;
    let flush = std::env::var("PIANISSIMO_FLUSH").as_deref() == Ok("1");
    let prepack = std::env::var("PIANISSIMO_NO_PREPACK").as_deref() != Ok("1");
    let mut hashes = serde_json::Map::new();
    for (file, expected) in [
        (
            "encoder-model.int8.onnx",
            "8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2",
        ),
        (
            "decoder_joint-model.int8.onnx",
            "2fb4ef1c1e28839aef70e74a3a2737afdc7460afa1e640ce1c4f9bf9ceadcb51",
        ),
        (
            "vocab.txt",
            "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        ),
        (
            "config.json",
            "666a05a8b9442a5f084df5915a89b050fed24b66498a04041bca7c951923ad66",
        ),
    ] {
        let actual = hash(&dir.join(file))?;
        ensure!(actual == expected, "Unverified model file: {file}");
        hashes.insert(file.into(), json!(actual));
    }
    // Validate every input before paying model initialization cost.
    let fixtures = args[3..]
        .iter()
        .map(|x| Ok((PathBuf::from(x), audio(Path::new(x))?)))
        .collect::<Result<Vec<_>>>()?;
    eprintln!("Loading model (ort 2.0.0-rc.12; CPU; threads={threads})");
    let parts = std::env::var_os("PIANISSIMO_ENCODER_PARTS").map(PathBuf::from);
    let shared_pool = std::env::var("PIANISSIMO_SHARED_POOL").as_deref() == Ok("1");
    if shared_pool {
        let pool = ort::environment::GlobalThreadPoolOptions::default()
            .with_intra_threads(threads)?
            .with_inter_threads(1)?
            .with_spin_control(true)?;
        ensure!(
            ort::init().with_global_thread_pool(pool).commit(),
            "ORT environment already initialized"
        );
    }
    let (mut model, load) = model::Model::load(dir, threads, flush, prepack, parts.as_deref())?;
    let mut report = json!({"engine":"Pianissimo Rust / CPU", "ort_crate":"2.0.0-rc.12",
        "runtime": ort::info(), "threads":threads, "flush_to_zero":flush,"prepacking":prepack,
        "encoder_parts":parts,"spinning":shared_pool || parts.is_none(),"shared_pool":shared_pool,
        "encoder_cpu_arena":parts.is_none(),
        "encoder_parts_sha256":parts.as_deref().map(hash).transpose()?,
        "avx2_precision":true,"model_sha256":hashes,"load_seconds":load.iter().sum::<f64>(),
        "load_encoder_seconds":load[0],"load_decoder_seconds":load[1],"cases":[]});
    for (path, samples) in &fixtures {
        let mut elapsed = Vec::new();
        let mut stages = Vec::new();
        let mut results = Vec::new();
        for _ in 0..3 {
            let start = Instant::now();
            let (result, timing) = model.transcribe(samples, || Ok(()))?;
            elapsed.push(start.elapsed().as_secs_f64());
            stages.push(timing);
            results.push(result);
        }
        ensure!(
            results.windows(2).all(|x| x[0] == x[1]),
            "Inconsistent repeat result: {}",
            path.display()
        );
        ensure!(
            results[0].timestamps.windows(2).all(|x| x[0] <= x[1])
                && results[0]
                    .timestamps
                    .iter()
                    .all(|&x| x >= 0.0 && x <= samples.len() as f64 / 16000.0),
            "Invalid token timestamps"
        );
        if samples.iter().all(|&x| x == 0.0) {
            ensure!(
                results[0].text.trim().is_empty(),
                "Hallucinated text on digital silence"
            );
        }
        let reference = if path.with_extension("txt").exists() {
            Some(fs::read_to_string(path.with_extension("txt"))?)
        } else {
            None
        };
        ensure!(
            !results[0].text.trim().is_empty()
                || reference.as_ref().is_none_or(|x| x.trim().is_empty()),
            "Empty transcript for reference speech"
        );
        let warm = (elapsed[1] + elapsed[2]) / 2.0;
        eprintln!("{}: warm {warm:.3}s — {}", path.display(), results[0].text);
        report["cases"].as_array_mut().unwrap().push(json!({
            "audio":path.canonicalize()?,"audio_sha256":hash(path)?,"audio_seconds":samples.len() as f64/16000.0,
            "reference":reference,"elapsed_seconds":elapsed,"warm_median_seconds":warm,
            "stages":stages,"result":results[0],"repeat_text_tokens_timestamps_equal":true,
            "token_timestamps_monotonic_and_in_range":true,
        }));
        fs::write(output, serde_json::to_vec_pretty(&report)?).context("Writing report")?;
    }
    // Check cancellation and absence of decoder-state leakage across different recordings.
    ensure!(
        model
            .transcribe(&fixtures[0].1, || anyhow::bail!("cancelled"))
            .is_err(),
        "Cancellation ignored"
    );
    let (again, _) = model.transcribe(&fixtures[0].1, || Ok(()))?;
    ensure!(
        json!(again) == report["cases"][0]["result"],
        "State leaked between recordings"
    );
    report["cross_recording_state_reset"] = json!(true);
    report["cancel_before_inference"] = json!(true);
    if parts.is_some() {
        let checks = std::cell::Cell::new(0);
        let cancelled = model.transcribe(&fixtures[0].1, || {
            checks.set(checks.get() + 1);
            ensure!(checks.get() < 4, "cancelled between encoder parts");
            Ok(())
        });
        ensure!(
            cancelled.is_err() && checks.get() == 4,
            "Encoder cancellation ignored"
        );
        report["cancel_between_encoder_parts"] = json!(true);
    }
    let checkpoints = std::cell::Cell::new(0);
    let cancel_at = 4 + model.encoder_stage_count();
    let cancelled = model.transcribe(&fixtures[0].1, || {
        checkpoints.set(checkpoints.get() + 1);
        ensure!(
            checkpoints.get() < cancel_at,
            "cancelled after decoder step"
        );
        Ok(())
    });
    ensure!(
        cancelled.is_err() && checkpoints.get() == cancel_at,
        "Decoder cancellation ignored"
    );
    let (after_cancel, _) = model.transcribe(&fixtures[0].1, || Ok(()))?;
    ensure!(
        json!(after_cancel) == report["cases"][0]["result"],
        "State leaked after cancellation"
    );
    report["cancel_during_decoding_and_reuse"] = json!(true);
    fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
