//! Meeting capture — two simultaneous WASAPI streams, written to WAV *and* chunked for live
//! transcription.
//!
//! The point: **your voice is the microphone** (`eCapture` default input) and **everyone else is
//! the system audio** (`eRender` default output, captured in loopback), so "Jag" vs "Mötet" is
//! separated on the *source* — no diarisation needed for that split.
//!
//! Each capture thread does two cheap things per read: (1) downmix to mono and append to a 16-bit
//! WAV at the native rate (the full recording, for playback + post-hoc diarisation), and (2) feed a
//! lightweight chunker that cuts a few-second segment at the quietest nearby frame and ships the
//! *native-rate* samples over a channel. A single worker (in `lib.rs`) resamples each chunk to
//! 16 kHz, transcribes it on the shared `Transcriber`, and emits a live utterance — so text appears
//! while the meeting is still going. Keeping resample+transcribe off the capture thread avoids
//! starving the WASAPI buffer (which would drop audio).
//!
//! WASAPI is Windows-only, so the real implementation is `#[cfg(windows)]` with a graceful stub on
//! other platforms (the project still compiles; the commands just return an error).

use std::sync::mpsc::Receiver;

/// Which stream a chunk came from. Maps to the speaker label "Jag" / "Mötet".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Mic,
    Meeting,
}

/// A few seconds of audio cut from one stream, tagged with its absolute start time (seconds from
/// recording start). Samples are at `src_rate`; the worker resamples to 16 kHz before transcribing.
pub struct CapturedChunk {
    pub source: Source,
    pub start_s: f64,
    pub samples: Vec<f32>,
    pub src_rate: u32,
}

/// Paths to the two finished source recordings plus the meeting's duration.
#[derive(Debug, Clone)]
pub struct MeetingFiles {
    /// WAV of the local microphone ("Jag").
    pub mic_wav: String,
    /// WAV of the system/render loopback ("Mötet").
    pub sys_wav: String,
    /// Longer of the two streams, in seconds.
    pub duration_s: f64,
}

#[cfg(windows)]
mod imp {
    use super::{CapturedChunk, MeetingFiles, Source};
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::Arc;
    use std::thread::{self, JoinHandle};
    use wasapi::*;

    // Chunker tuning. Cut at the quietest 30 ms frame between MIN and the buffer end once we reach
    // TARGET; force a cut at MAX so latency stays bounded even through nonstop speech.
    const MIN_S: f64 = 2.5;
    const TARGET_S: f64 = 6.0;
    const MAX_S: f64 = 11.0;
    const FRAME_S: f64 = 0.03;
    /// A chunk whose loudest sample is below this is treated as silence and not transcribed
    /// (Whisper hallucinates on silence). Tunable.
    const SILENCE_FLOOR: f32 = 0.015;

    fn es<E: std::fmt::Display>(e: E) -> String {
        e.to_string()
    }

    fn max_abs(s: &[f32]) -> f32 {
        s.iter().fold(0f32, |m, &x| m.max(x.abs()))
    }

    fn rms(s: &[f32]) -> f32 {
        if s.is_empty() {
            return 0.0;
        }
        (s.iter().map(|&x| x * x).sum::<f32>() / s.len() as f32).sqrt()
    }

    /// Accumulates native-rate mono samples and emits speech chunks cut at quiet points.
    struct Chunker {
        rate: u32,
        source: Source,
        buf: Vec<f32>,
        start_samples: u64, // absolute index of buf[0] since recording start
    }

    impl Chunker {
        fn new(rate: u32, source: Source) -> Self {
            Chunker { rate, source, buf: Vec::new(), start_samples: 0 }
        }

        fn push(&mut self, mono: &[f32], tx: &Sender<CapturedChunk>) {
            self.buf.extend_from_slice(mono);
            let target = (self.rate as f64 * TARGET_S) as usize;
            while self.buf.len() >= target {
                let cut = self.choose_cut();
                self.emit(cut, tx);
            }
        }

        /// Sample index to cut at: the start of the quietest frame in [MIN, len], or MAX when the
        /// buffer is already that long (forced cut through continuous speech).
        fn choose_cut(&self) -> usize {
            let len = self.buf.len();
            let max = (self.rate as f64 * MAX_S) as usize;
            if len >= max {
                return max;
            }
            let min = (self.rate as f64 * MIN_S) as usize;
            let frame = ((self.rate as f64 * FRAME_S) as usize).max(1);
            let mut best = len;
            let mut best_rms = f32::MAX;
            let mut i = min;
            while i + frame <= len {
                let r = rms(&self.buf[i..i + frame]);
                if r < best_rms {
                    best_rms = r;
                    best = i;
                }
                i += frame;
            }
            best.clamp(min.min(len), len)
        }

        fn emit(&mut self, cut: usize, tx: &Sender<CapturedChunk>) {
            let cut = cut.min(self.buf.len());
            if cut == 0 {
                return;
            }
            let chunk: Vec<f32> = self.buf.drain(..cut).collect();
            let start_s = self.start_samples as f64 / self.rate as f64;
            self.start_samples += cut as u64;
            if max_abs(&chunk) > SILENCE_FLOOR {
                let _ = tx.send(CapturedChunk { source: self.source, start_s, samples: chunk, src_rate: self.rate });
            }
        }

        /// Ship the trailing partial chunk (if ≥1 s) when recording stops.
        fn flush(&mut self, tx: &Sender<CapturedChunk>) {
            if self.buf.len() >= self.rate as usize {
                let cut = self.buf.len();
                self.emit(cut, tx);
            }
        }
    }

    /// A live dual-stream recording. Two capture threads run until [`stop`](Self::stop); each owns
    /// its WASAPI client + WAV writer (COM objects stay on their own thread) and a chunk sender.
    pub struct MeetingCapture {
        stop: Arc<AtomicBool>,
        mic: Option<JoinHandle<(u64, u32)>>,
        sys: Option<JoinHandle<(u64, u32)>>,
        mic_wav: PathBuf,
        sys_wav: PathBuf,
    }

    impl MeetingCapture {
        /// Open both endpoints and start recording. Returns the capture handle and a receiver of
        /// live chunks (drained by the worker in `lib.rs`). Errors synchronously if either stream
        /// fails to open, so the UI learns immediately rather than after a wasted meeting.
        pub fn start(mic_wav: PathBuf, sys_wav: PathBuf) -> Result<(MeetingCapture, Receiver<CapturedChunk>), String> {
            let stop = Arc::new(AtomicBool::new(false));
            let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
            let (chunk_tx, chunk_rx) = mpsc::channel::<CapturedChunk>();

            let mic = {
                let (wav, stop, rt, ct) = (mic_wav.clone(), stop.clone(), ready_tx.clone(), chunk_tx.clone());
                thread::spawn(move || run_stream(Source::Mic, Direction::Capture, wav, stop, rt, ct))
            };
            let sys = {
                let (wav, stop, rt, ct) = (sys_wav.clone(), stop.clone(), ready_tx.clone(), chunk_tx.clone());
                thread::spawn(move || run_stream(Source::Meeting, Direction::Render, wav, stop, rt, ct))
            };
            drop(ready_tx);
            drop(chunk_tx); // only the threads hold senders now → channel closes when both end

            let mut errs = Vec::new();
            for _ in 0..2 {
                match ready_rx.recv() {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => errs.push(e),
                    Err(_) => errs.push("inspelningstråden avslutades innan den startade".to_string()),
                }
            }
            if !errs.is_empty() {
                stop.store(true, Ordering::Relaxed);
                let _ = mic.join();
                let _ = sys.join();
                return Err(errs.join("; "));
            }
            Ok((MeetingCapture { stop, mic: Some(mic), sys: Some(sys), mic_wav, sys_wav }, chunk_rx))
        }

        /// Signal both threads to stop, wait for the WAVs to finalise (and the last chunks to be
        /// sent), and report the file paths + duration. Dropping the threads' chunk senders closes
        /// the worker's channel.
        pub fn stop(mut self) -> Result<MeetingFiles, String> {
            self.stop.store(true, Ordering::Relaxed);
            let (mf, mr) = self.mic.take().and_then(|h| h.join().ok()).unwrap_or((0, 0));
            let (sf, sr) = self.sys.take().and_then(|h| h.join().ok()).unwrap_or((0, 0));
            let mic_dur = mf as f64 / mr.max(1) as f64;
            let sys_dur = sf as f64 / sr.max(1) as f64;
            Ok(MeetingFiles {
                mic_wav: self.mic_wav.to_string_lossy().to_string(),
                sys_wav: self.sys_wav.to_string_lossy().to_string(),
                duration_s: mic_dur.max(sys_dur),
            })
        }
    }

    /// Capture loop for one endpoint. `device_dir = Capture` → microphone; `device_dir = Render` →
    /// system loopback (a Render *device* with a Capture *stream* sets AUDCLNT_STREAMFLAGS_LOOPBACK
    /// inside the wasapi crate). Returns `(frames_written, sample_rate)`.
    fn run_stream(
        source: Source,
        device_dir: Direction,
        wav_path: PathBuf,
        stop: Arc<AtomicBool>,
        ready: Sender<Result<(), String>>,
        chunks: Sender<CapturedChunk>,
    ) -> (u64, u32) {
        let opened = (|| -> Result<_, String> {
            let _ = initialize_mta();
            let enumerator = DeviceEnumerator::new().map_err(es)?;
            let device = enumerator.get_default_device(&device_dir).map_err(es)?;
            let mut client = device.get_iaudioclient().map_err(es)?;
            // Request f32 stereo @48k; autoconvert makes WASAPI deliver exactly this regardless of
            // the endpoint's native format (the A0 spike confirmed 48000 Hz / 2 ch for both).
            let format = WaveFormat::new(32, 32, &SampleType::Float, 48000, 2, None);
            let (_def, min_time) = client.get_device_period().map_err(es)?;
            let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: min_time };
            client.initialize_client(&format, &Direction::Capture, &mode).map_err(es)?;
            let h_event = client.set_get_eventhandle().map_err(es)?;
            let capture_client = client.get_audiocaptureclient().map_err(es)?;
            let rate = format.get_samplespersec();
            let channels = format.get_nchannels() as usize;
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let writer = hound::WavWriter::create(&wav_path, spec).map_err(es)?;
            client.start_stream().map_err(es)?;
            Ok((client, h_event, capture_client, channels, rate, writer))
        })();

        let (client, h_event, capture_client, channels, rate, mut writer) = match opened {
            Ok(v) => {
                let _ = ready.send(Ok(()));
                v
            }
            Err(e) => {
                let _ = ready.send(Err(e));
                return (0, 0);
            }
        };

        let frame_bytes = 4 * channels.max(1);
        let mut queue: VecDeque<u8> = VecDeque::new();
        let mut chunker = Chunker::new(rate, source);
        let mut frames: u64 = 0;
        let mut fault: Option<String> = None;

        while !stop.load(Ordering::Relaxed) {
            if let Err(e) = capture_client.read_from_device_to_deque(&mut queue) {
                fault = Some(format!("läsfel: {e}"));
                break;
            }
            // Drain whole interleaved f32 frames → mono; write 16-bit PCM and buffer for chunking.
            let mut batch: Vec<f32> = Vec::with_capacity(queue.len() / frame_bytes + 1);
            while queue.len() >= frame_bytes {
                let mut acc = 0f32;
                for _ in 0..channels {
                    let b = [
                        queue.pop_front().unwrap(),
                        queue.pop_front().unwrap(),
                        queue.pop_front().unwrap(),
                        queue.pop_front().unwrap(),
                    ];
                    acc += f32::from_le_bytes(b);
                }
                let mono = (acc / channels as f32).clamp(-1.0, 1.0);
                if let Err(e) = writer.write_sample((mono * 32767.0) as i16) {
                    fault = Some(format!("WAV-skrivfel: {e}"));
                    break;
                }
                batch.push(mono);
                frames += 1;
            }
            if fault.is_some() {
                break;
            }
            chunker.push(&batch, &chunks);
            // Loopback is silent when nothing plays → the event simply times out; keep recording.
            let _ = h_event.wait_for_event(200);
        }

        chunker.flush(&chunks);
        let _ = client.stop_stream();
        let _ = writer.finalize();
        if let Some(e) = fault {
            eprintln!("[capture] {}: {e}", wav_path.display());
        }
        (frames, rate)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{CapturedChunk, MeetingFiles};
    use std::path::PathBuf;
    use std::sync::mpsc::Receiver;

    /// Non-Windows stub: meeting capture relies on WASAPI loopback, which only exists on Windows.
    pub struct MeetingCapture;

    impl MeetingCapture {
        pub fn start(
            _mic_wav: PathBuf,
            _sys_wav: PathBuf,
        ) -> Result<(MeetingCapture, Receiver<CapturedChunk>), String> {
            Err("Mötesinspelning stöds endast på Windows (kräver WASAPI-loopback).".to_string())
        }
        pub fn stop(self) -> Result<MeetingFiles, String> {
            Err("Mötesinspelning stöds endast på Windows.".to_string())
        }
    }
}

pub use imp::MeetingCapture;

// Re-export so callers can name the receiver type without importing std::sync::mpsc directly.
pub type ChunkReceiver = Receiver<CapturedChunk>;

#[cfg(all(windows, test))]
mod spike {
    use std::collections::VecDeque;
    use std::time::Instant;
    use wasapi::*;

    /// Capture ~`secs` seconds from the default device of `device_dir`.
    /// `device_dir = Render` + a Capture stream = system loopback; `device_dir = Capture` = mic.
    /// Returns (read_iterations, max_abs_f32_sample, sample_rate, channels).
    fn capture(device_dir: Direction, secs: f64) -> Result<(usize, f32, usize, usize), Box<dyn std::error::Error>> {
        let enumerator = DeviceEnumerator::new()?;
        let device = enumerator.get_default_device(&device_dir)?;
        let mut client = device.get_iaudioclient()?;

        let format = WaveFormat::new(32, 32, &SampleType::Float, 48000, 2, None);
        let (_def_time, min_time) = client.get_device_period()?;
        let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: min_time };
        client.initialize_client(&format, &Direction::Capture, &mode)?;

        let h_event = client.set_get_eventhandle()?;
        let capture_client = client.get_audiocaptureclient()?;

        let mut queue: VecDeque<u8> = VecDeque::new();
        let mut max_abs = 0f32;
        let mut reads = 0usize;

        client.start_stream()?;
        let start = Instant::now();
        while start.elapsed().as_secs_f64() < secs {
            capture_client.read_from_device_to_deque(&mut queue)?;
            reads += 1;
            while queue.len() >= 4 {
                let b = [
                    queue.pop_front().unwrap(),
                    queue.pop_front().unwrap(),
                    queue.pop_front().unwrap(),
                    queue.pop_front().unwrap(),
                ];
                let s = f32::from_le_bytes(b).abs();
                if s > max_abs {
                    max_abs = s;
                }
            }
            let _ = h_event.wait_for_event(1000);
        }
        client.stop_stream()?;

        let rate = format.get_samplespersec() as usize;
        let channels = format.get_nchannels() as usize;
        Ok((reads, max_abs, rate, channels))
    }

    #[test]
    #[ignore]
    fn dual_capture() {
        initialize_mta().ok().expect("COM init");

        eprintln!("\n=== SYSTEM LOOPBACK (spela upp ljud nu!) — 5 s ===");
        match capture(Direction::Render, 5.0) {
            Ok((reads, max_abs, rate, ch)) => eprintln!(
                "loopback: reads={reads} max_abs={max_abs:.4} fmt={rate}Hz/{ch}ch  => {}",
                if max_abs > 0.0001 { "OK — fångade ljud" } else { "TYST (spelades något upp?)" }
            ),
            Err(e) => eprintln!("loopback MISSLYCKADES: {e}"),
        }

        eprintln!("\n=== MIKROFON (säg något) — 3 s ===");
        match capture(Direction::Capture, 3.0) {
            Ok((reads, max_abs, rate, ch)) => eprintln!(
                "mic: reads={reads} max_abs={max_abs:.4} fmt={rate}Hz/{ch}ch  => {}",
                if max_abs > 0.0001 { "OK — fångade ljud" } else { "TYST" }
            ),
            Err(e) => eprintln!("mic MISSLYCKADES: {e}"),
        }
        eprintln!();
    }
}
