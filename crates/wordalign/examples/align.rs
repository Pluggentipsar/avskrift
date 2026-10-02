//! Align a Whisper word list (textedit-probe JSON) against 16 kHz mono PCM16 audio.
//! Usage: align MODEL.onnx VOCAB.json AUDIO16K.wav WHISPER.json OUT.json cpu|dml
use std::{env, fs, path::Path, time::Instant};

use anyhow::{bail, Result};
use avskrift_wordalign::{align_words, Device, Emitter, InputWord, Vocab};

fn main() -> Result<()> {
    let a: Vec<String> = env::args().collect();
    if a.len() != 7 {
        bail!("Usage: align MODEL.onnx VOCAB.json AUDIO16K.wav WHISPER.json OUT.json cpu|dml");
    }
    let device = if a[6] == "dml" { Device::DirectMl } else { Device::Cpu };
    let vocab = Vocab::from_json(&fs::read_to_string(&a[2])?)?;
    let mut reader = hound::WavReader::open(&a[3])?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_rate != 16000 || spec.bits_per_sample != 16 {
        bail!("Input must be mono 16 kHz PCM16 WAV");
    }
    let audio: Vec<f32> = reader.samples::<i16>().map(|s| s.map(|v| v as f32 / 32768.0)).collect::<Result<_, _>>()?;
    let whisper: serde_json::Value = serde_json::from_str(&fs::read_to_string(&a[4])?)?;
    let words: Vec<InputWord> = serde_json::from_value(whisper["words"].clone())?;

    let t = Instant::now();
    let mut emitter = Emitter::load(Path::new(&a[1]), device, 8)?;
    let load_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let em = emitter.emissions(&audio, |_| true)?;
    let emit_s = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let aligned = align_words(&em, &vocab, &words);
    let align_s = t.elapsed().as_secs_f64();
    let out: Vec<_> = aligned
        .iter()
        .zip(&words)
        .map(|(w, i)| {
            serde_json::json!({"text": w.text, "start": w.start, "end": w.end, "score": w.score,
                "aligned": w.aligned, "whisper_start": i.start, "whisper_end": i.end})
        })
        .collect();
    let report = serde_json::json!({
        "device": a[6], "audio_seconds": audio.len() as f64 / 16000.0, "load_seconds": load_s,
        "emission_seconds": emit_s, "align_seconds": align_s, "words": out,
    });
    fs::write(&a[5], serde_json::to_string_pretty(&report)?)?;
    println!(
        "{}: load {load_s:.1}s, emissions {emit_s:.1}s, align {align_s:.2}s, {} words, {} unaligned",
        a[6],
        aligned.len(),
        aligned.iter().filter(|w| !w.aligned).count()
    );
    Ok(())
}
