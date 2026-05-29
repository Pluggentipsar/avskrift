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
    /// Length in seconds (convenience for progress / SRT bounds).
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
    let src_sr = track.codec_params.sample_rate.unwrap_or(TARGET_SR);
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1).max(1);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow!("ingen avkodare för ljudkodeken: {e}"))?;

    // Decode all packets into an interleaved-then-downmixed mono buffer at the source rate.
    let mut mono: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(e) => return Err(anyhow!("fel vid läsning av ljud: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => append_mono(&decoded, channels, &mut mono),
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue, // skip a bad frame
            Err(e) => return Err(anyhow!("avkodningsfel: {e}")),
        }
    }

    let samples = if src_sr == TARGET_SR {
        mono
    } else {
        resample(&mono, src_sr, TARGET_SR)?
    };
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

/// High-quality resample from `from_sr` to `to_sr` using a sinc interpolator.
fn resample(input: &[f32], from_sr: u32, to_sr: u32) -> Result<Vec<f32>> {
    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };

    if input.is_empty() {
        return Ok(Vec::new());
    }

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };
    let ratio = to_sr as f64 / from_sr as f64;
    let chunk = 1024usize;
    let mut resampler = SincFixedIn::<f32>::new(ratio, 2.0, params, chunk, 1)
        .map_err(|e| anyhow!("kunde inte skapa resampler: {e}"))?;

    let mut out: Vec<f32> = Vec::with_capacity((input.len() as f64 * ratio) as usize + chunk);
    let mut pos = 0usize;
    while pos + chunk <= input.len() {
        let frame = vec![input[pos..pos + chunk].to_vec()];
        let res = resampler
            .process(&frame, None)
            .map_err(|e| anyhow!("resampling misslyckades: {e}"))?;
        out.extend_from_slice(&res[0]);
        pos += chunk;
    }
    // Tail: pad the last partial chunk with zeros.
    if pos < input.len() {
        let mut last = input[pos..].to_vec();
        last.resize(chunk, 0.0);
        if let Ok(res) = resampler.process(&[last], None) {
            out.extend_from_slice(&res[0]);
        }
    }
    Ok(out)
}
