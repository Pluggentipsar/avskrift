//! Opt-in native preparation/integration check using explicitly supplied local PCM fixtures.
use anyhow::{ensure, Result};
use avskrift_pianissimo as p;
use std::{path::Path, time::Instant};
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(args.len() >= 4, "MODEL_DIR REPORT.json PCM16_WAV...");
    let dir = Path::new(&args[1]);
    let pool = ort::environment::GlobalThreadPoolOptions::default()
        .with_intra_threads(8)?
        .with_inter_threads(1)?
        .with_spin_control(true)?;
    ensure!(
        ort::init().with_global_thread_pool(pool).commit(),
        "ORT already initialized"
    );
    let start = Instant::now();
    let manifest = p::prepare::prepare(dir, 8, &|s| eprintln!("{s}"), &|| Ok(()))?;
    let prepared = start.elapsed().as_secs_f64();
    let start = Instant::now();
    let (mut model, _) = p::model::Model::load(dir, 8, false, true, Some(&manifest))?;
    let loaded = start.elapsed().as_secs_f64();
    let mut cases = Vec::new();
    for path in &args[3..] {
        let mut reader = hound::WavReader::open(path)?;
        ensure!(
            reader.spec().channels == 1
                && reader.spec().sample_rate == 16000
                && reader.spec().bits_per_sample == 16,
            "Expected PCM16 mono 16k"
        );
        let samples = reader
            .samples::<i16>()
            .map(|s| s.map(|s| s as f32 / 32768.0))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let start = Instant::now();
        let direct = if samples.len() <= 36 * 16000 {
            Some(model.transcribe(&samples, || Ok(()))?.0)
        } else {
            None
        };
        let segments = p::segments::transcribe(&mut model, &samples, || Ok(()), |_| {})?;
        ensure!(
            segments.iter().all(|s| s.start >= 0.0
                && s.end >= s.start
                && s.end <= samples.len() as f64 / 16000.0),
            "Timestamp range"
        );
        ensure!(
            segments.windows(2).all(|w| w[0].end <= w[1].start),
            "Timeline overlap"
        );
        if samples.len() <= 36 * 16000 {
            ensure!(
                segments
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
                    == direct.as_ref().unwrap().text.trim(),
                "Segment text differs"
            );
        }
        cases.push(serde_json::json!({"audio":path,"sha256":p::hash(Path::new(path))?,"direct":direct,"segments":segments,"seconds":start.elapsed().as_secs_f64()}));
    }
    let cancelled = p::segments::transcribe(
        &mut model,
        &[0.0; 16000],
        || anyhow::bail!("cancelled"),
        |_| {},
    );
    ensure!(
        cancelled.unwrap_err().to_string() == "cancelled",
        "Cancellation ignored"
    );
    ensure!(
        p::segments::transcribe(&mut model, &[0.0; 16000], || Ok(()), |_| {})?.is_empty(),
        "State leaked after cancel"
    );
    std::fs::write(
        &args[2],
        serde_json::to_vec_pretty(
            &serde_json::json!({"preparation_seconds":prepared,"load_seconds":loaded,"cases":cases,"cancellation":true}),
        )?,
    )?;
    Ok(())
}
