//! FFmpeg command lines. The app ships an LGPL FFmpeg build without x264, so H.264 comes from a
//! hardware encoder or Windows Media Foundation.

/// H.264 encoders in order of preference. Hardware encoders may be compiled in but unusable
/// (no such GPU); the app test-encodes one frame with each before choosing.
pub const H264_ENCODERS: [&str; 4] = ["h264_nvenc", "h264_amf", "h264_qsv", "h264_mf"];

/// Proxy height; playback only, never exported.
pub const PROXY_HEIGHT: u32 = 720;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// One-frame test encode; succeeds only when `encoder` really works on this machine.
pub fn encoder_probe_args(encoder: &str) -> Vec<String> {
    s(&["-hide_banner", "-v", "error", "-f", "lavfi", "-i", "color=c=black:s=256x144:r=25", "-frames:v", "2", "-c:v", encoder, "-f", "null", "-"])
}

/// Quality options per encoder, roughly equivalent to CRF 23 for a 720p proxy.
fn quality(encoder: &str) -> Vec<String> {
    match encoder {
        "h264_nvenc" => s(&["-preset", "p4", "-rc", "vbr", "-cq", "26", "-b:v", "0"]),
        "h264_amf" => s(&["-quality", "speed", "-rc", "cqp", "-qp_i", "24", "-qp_p", "26"]),
        "h264_qsv" => s(&["-preset", "faster", "-global_quality", "26"]),
        _ => s(&["-b:v", "2500k"]), // h264_mf: bitrate mode is the reliable one
    }
}

/// 16 kHz mono PCM16 WAV for transcription and alignment.
pub fn audio_args(src: &str, dst: &str) -> Vec<String> {
    let mut a = s(&["-hide_banner", "-v", "error", "-nostdin", "-y", "-i", src, "-vn", "-ac", "1", "-ar", "16000"]);
    a.extend(s(&["-c:a", "pcm_s16le", "-progress", "pipe:1", "-nostats", dst]));
    a
}

/// Playback proxy: 720p H.264, constant frame rate (the source's nominal rate, so variable-rate
/// phone video gets one stable timeline), a keyframe every half second for instant seeks, AAC
/// audio, fast start. Rotation is applied by FFmpeg's autorotate, so the proxy plays upright.
pub fn proxy_args(src: &str, dst: &str, encoder: &str, fps: f64) -> Vec<String> {
    let fps = if fps > 0.0 && fps <= 120.0 { fps } else { 30.0 };
    let gop = ((fps / 2.0).round() as u32).max(1).to_string();
    let mut a = s(&["-hide_banner", "-v", "error", "-nostdin", "-y", "-i", src]);
    a.extend(s(&["-vf", &format!("scale=-2:{PROXY_HEIGHT}:flags=bicubic,format=yuv420p"), "-fps_mode", "cfr"]));
    a.extend(s(&["-r", &format!("{fps}"), "-c:v", encoder]));
    a.extend(quality(encoder));
    a.extend(s(&["-g", &gop, "-c:a", "aac", "-b:a", "128k", "-ac", "2", "-movflags", "+faststart"]));
    a.extend(s(&["-progress", "pipe:1", "-nostats", dst]));
    a
}

/// Parse `-progress pipe:1` output; returns processed seconds from an `out_time_us=` line.
pub fn progress_seconds(line: &str) -> Option<f64> {
    let v = line.strip_prefix("out_time_us=").or_else(|| line.strip_prefix("out_time_ms="))?;
    v.trim().parse::<i64>().ok().filter(|us| *us >= 0).map(|us| us as f64 / 1e6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_has_dense_keyframes_and_constant_rate() {
        let a = proxy_args("in.mov", "out.mp4", "h264_nvenc", 50.0).join(" ");
        assert!(a.contains("-g 25"), "{a}");
        assert!(a.contains("-fps_mode cfr") && a.contains("-r 50"));
        assert!(a.contains("scale=-2:720"));
        assert!(a.ends_with("out.mp4"));
        // Nonsense rates fall back to 30 fps.
        assert!(proxy_args("a", "b", "h264_mf", 90000.0).join(" ").contains("-r 30 "));
    }

    #[test]
    fn progress_lines() {
        assert_eq!(progress_seconds("out_time_us=1500000"), Some(1.5));
        assert_eq!(progress_seconds("out_time_us=N/A"), None);
        assert_eq!(progress_seconds("frame=10"), None);
    }
}
