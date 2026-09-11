//! Audio decoding to the format Whisper and the diariser expect: 16 kHz, mono, f32 PCM in [-1, 1].
//!
//! Uses `symphonia` for demux/decode (mp3, wav, flac, ogg/vorbis, m4a/aac, …) and `rubato` for
//! high-quality resampling — no external FFmpeg dependency, so the single-binary install holds.

use std::fs::File;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Target sample rate for Whisper / pyannote.
pub const TARGET_SR: u32 = 16_000;

/// Decoded, resampled audio ready for inference.
pub struct Audio {
    /// Mono f32 samples in [-1, 1] at [`TARGET_SR`].
    pub samples: Vec<f32>,
    /// Length in seconds (convenience for progress / SRT bounds). Computed on load; not yet read.
    #[allow(dead_code)]
    pub duration_s: f64,
}

/// Decode `path` to mono 16 kHz f32. Mixes down multi-channel audio by averaging channels.
pub fn load(path: &Path) -> Result<Audio> {
    let file = File::open(path).with_context(|| format!("kunde inte öppna {}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow!("filformatet kunde inte tolkas: {e}"))?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow!("ingen ljudström hittades i filen"))?;
    let track_id = track.id;


    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("ingen avkodare för ljudkodeken: {e}"))?;

    // Only a decoded packet and a fixed resampling block remain at the source rate.
    let mut stream: Option<MonoResampler> = None;
    let mut layout = None;
    let mut mono = Vec::new();
    loop {
        crate::work::check()?;
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(anyhow!("fel vid läsning av ljud: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                let current = (spec.rate, spec.channels);
                anyhow::ensure!(layout.is_none_or(|v| v == current), "Ljudformatet ändras mitt i filen. Exportera filen med en fast samplingsfrekvens.");
                layout = Some(current);
                if stream.is_none() { stream = Some(MonoResampler::new(spec.rate, TARGET_SR)?); }
                mono.clear();
                append_mono(&decoded, spec.channels.count(), &mut mono);
                stream.as_mut().unwrap().push(&mono)?;
            },
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue, // skip a bad frame
            Err(e) => return Err(anyhow!("avkodningsfel: {e}")),
        }
    }

    let samples = match stream { Some(stream) => stream.finish()?, None => Vec::new() };
    let duration_s = samples.len() as f64 / TARGET_SR as f64;
    Ok(Audio { samples, duration_s })
}

/// Downmix one decoded buffer to mono and append to `out`. Converts any sample format to f32 via
/// an interleaved `SampleBuffer`, then averages channels.
fn append_mono(decoded: &AudioBufferRef, channels: usize, out: &mut Vec<f32>) {
    let mut sample_buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
    sample_buf.copy_interleaved_ref(decoded.clone());
    let interleaved = sample_buf.samples();
    out.reserve(interleaved.len() / channels.max(1));
    for frame in interleaved.chunks(channels.max(1)) {
        let acc: f32 = frame.iter().sum();
        out.push(acc / channels as f32);
    }
}

use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};
const BLOCK: usize = 1024;

/// Stateful across decoder packets. Source-rate staging is bounded by BLOCK; the final 16 kHz
/// output is still retained because inference and diarisation consume a contiguous slice.
struct MonoResampler {
    engine: Option<SincFixedIn<f32>>, pending: Vec<f32>, buffer: Vec<Vec<f32>>,
    output: Vec<f32>, source_frames: usize, from: u32, to: u32,
}
impl MonoResampler {
    fn new(from: u32, to: u32) -> Result<Self> {
        anyhow::ensure!(from > 0 && to > 0, "Ogiltig samplingsfrekvens.");
        let engine = if from == to { None } else { Some(SincFixedIn::new(to as f64 / from as f64, 2.0,
            SincInterpolationParameters { sinc_len: 256, f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear, oversampling_factor: 256,
                window: WindowFunction::BlackmanHarris2 }, BLOCK, 1)?) };
        let buffer = engine.as_ref().map(|r| r.output_buffer_allocate(true)).unwrap_or_default();
        // rubato 0.15 SincFixedIn starts its interpolation index at -sinc_len/2.
        // Its output is already aligned (within one output sample); output_delay() describes
        // lookahead, not leading silence in this pinned implementation. See impulse tests.
        Ok(Self { engine, pending: Vec::with_capacity(BLOCK), buffer, output: Vec::new(), source_frames: 0, from, to })
    }
    fn block(&mut self) -> Result<()> {
        crate::work::check()?;
        let (_, n) = self.engine.as_mut().unwrap().process_into_buffer(&[&self.pending], &mut self.buffer, None)?;
        self.output.extend_from_slice(&self.buffer[0][..n]);
        self.pending.clear();
        Ok(())
    }
    fn push(&mut self, mut input: &[f32]) -> Result<()> {
        crate::work::check()?;
        self.source_frames += input.len();
        if self.engine.is_none() { self.output.extend_from_slice(input); return Ok(()); }
        while !input.is_empty() {
            let n = (BLOCK - self.pending.len()).min(input.len());
            self.pending.extend_from_slice(&input[..n]); input = &input[n..];
            if self.pending.len() == BLOCK { self.block()?; }
        }
        Ok(())
    }
    fn finish(mut self) -> Result<Vec<f32>> {
        let target = ((self.source_frames as u128 * self.to as u128 + self.from as u128 / 2) / self.from as u128) as usize;
        if self.engine.is_some() {
            // Flush filter history, including when input ended exactly on a block boundary.
            while self.output.len() < target {
                self.pending.resize(BLOCK, 0.0); self.block()?;
            }
        }
        self.output.truncate(target);
        Ok(self.output)
    }
}
fn resample(input: &[f32], from_sr: u32, to_sr: u32) -> Result<Vec<f32>> {
    let mut stream = MonoResampler::new(from_sr, to_sr)?;
    stream.push(input)?;
    stream.finish()
}

/// Mix interleaved multi-channel f32 frames down to mono by averaging channels. A trailing partial
/// frame (fewer than `channels` samples) is dropped. Kept as a shared helper for interleaved WASAPI
/// frames; the live meeting capture currently inlines its own downmix, so this is not called yet.
#[allow(dead_code)]
pub fn downmix_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    let ch = channels.max(1);
    let mut out = Vec::with_capacity(interleaved.len() / ch);
    for frame in interleaved.chunks_exact(ch) {
        let acc: f32 = frame.iter().sum();
        out.push(acc / ch as f32);
    }
    out
}

/// Resample mono f32 `input` from `src_sr` to [`TARGET_SR`] (16 kHz). One-shot over the whole
/// buffer; returns the input unchanged when it is already at the target rate. Used by the live
/// meeting capture to feed Whisper directly from in-memory chunks (no WAV round-trip).
pub fn resample_to_16k(input: &[f32], src_sr: u32) -> Vec<f32> {
    if src_sr == TARGET_SR || input.is_empty() {
        return input.to_vec();
    }
    resample(input, src_sr, TARGET_SR).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packet_boundaries_preserve_samples_length_and_tail() {
        for rate in [8000, 16000, 44100, 48000, 96000] {
            for len in [0, 37, 1024, 3197, 48000] {
                let input: Vec<f32> = (0..len).map(|i| (i as f32 / 13.0).sin() * 0.4).collect();
                let expected = resample(&input, rate, TARGET_SR).unwrap();
                assert_eq!(expected.len(), (len as f64 * 16000.0 / rate as f64).round() as usize);
                for packet in [1, 137, 1024, 4096] {
                    let mut r = MonoResampler::new(rate, TARGET_SR).unwrap();
                    for part in input.chunks(packet) { r.push(part).unwrap(); assert!(r.pending.len() < BLOCK); }
                    assert_eq!(expected, r.finish().unwrap());
                }
            }
        }
    }
    #[test]
    fn impulses_have_no_filter_delay_at_start_block_boundary_or_tail() {
        for rate in [8000, 44100, 48000, 96000] {
            for position in [0, 1023, 1024, 2047, 3196] {
                let mut input = vec![0.0; 3197]; input[position] = 1.0;
                let out = resample(&input, rate, TARGET_SR).unwrap();
                let peak = out.iter().enumerate().max_by(|a,b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0;
                let expected = (position as f64 * 16000.0 / rate as f64).round() as isize;
                assert!((peak as isize - expected).abs() <= 1, "rate={rate} position={position} peak={peak} expected={expected}");
                assert!(out[peak].abs() > 0.01, "rate={rate} position={position} peak={peak} amplitude={}",out[peak]);
            }
        }
    }
    #[test]
    fn decoded_stereo_wav_matches_mono_resampling() {
        let path = std::env::temp_dir().join(format!("avskrift-stream-{}.wav",std::process::id()));
        let spec = hound::WavSpec { channels: 2, sample_rate: 48000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut writer = hound::WavWriter::create(&path,spec).unwrap();
        let input: Vec<f32> = (0..100_003).map(|i| (i as f32 / 37.0).sin() * 0.4).collect();
        for x in &input { writer.write_sample(*x).unwrap(); writer.write_sample(0.0f32).unwrap(); }
        writer.finalize().unwrap();
        let actual = load(&path).unwrap(); std::fs::remove_file(path).unwrap();
        let mono: Vec<_> = input.iter().map(|x| x / 2.0).collect();
        assert_eq!(actual.samples, resample(&mono,48000,TARGET_SR).unwrap());
        assert_eq!(actual.duration_s, actual.samples.len() as f64 / 16000.0);
    }
}
