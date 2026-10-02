//! Export plan: how the kept ranges become one file.
//!
//! Each kept piece is read with its own exact input seek (`-ss a -t len -i src`), so FFmpeg never
//! buffers the stretches in between (one filter graph over a whole hour would). Pieces are encoded
//! in groups into intermediate Matroska files with **PCM audio**; the final step concatenates them
//! with the video stream copied and encodes the audio once. Encoding AAC per group would add the
//! encoder's priming silence at every join and drift the sound against the picture.

use serde::{Deserialize, Serialize};

/// Pieces per intermediate file: bounds open decoders per FFmpeg run.
pub const GROUP: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Quality {
    /// Visually close to the original.
    High,
    /// About a third of the size, still sharp for talking heads.
    Small,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub start: f64,
    pub end: f64,
    /// Whole video frames in the piece (0 for audio-only). Picture and sound are both made exactly
    /// this long; otherwise FFmpeg's concat pads the shorter stream and the sound drifts at every
    /// join (measured: up to 150 ms after 50 joins before this was fixed).
    pub frames: u64,
    /// Audio fade at the piece's start and end, seconds (0 at the file's own start/end).
    pub fade_in: f64,
    pub fade_out: f64,
}

/// Fade length at a join: short when the cut sits in silence, longer when it cuts through sound.
pub fn fade_for(quiet: bool) -> f64 {
    if quiet {
        0.010
    } else {
        0.025
    }
}

/// Pieces from kept ranges; `quiet(t)` tells whether the audio at a cut point is silent. With a
/// frame rate, boundaries snap to whole frames and empty pieces are dropped.
pub fn pieces(keep: &[(f64, f64)], duration: f64, fps: Option<f64>, quiet: impl Fn(f64) -> bool) -> Vec<Piece> {
    keep.iter()
        .filter_map(|&(a, b)| {
            let (start, end, frames) = match fps {
                Some(fps) => {
                    let (fa, fb) = ((a * fps).round(), (b * fps).round());
                    (fa / fps, fb / fps, (fb - fa).max(0.0) as u64)
                }
                None => (a, b, 0),
            };
            if end - start < 0.02 || (fps.is_some() && frames == 0) {
                return None;
            }
            Some(Piece {
                start,
                end,
                frames,
                fade_in: if a <= 0.001 { 0.0 } else { fade_for(quiet(a)) },
                fade_out: if b >= duration - 0.001 { 0.0 } else { fade_for(quiet(b)) },
            })
        })
        .collect()
}

/// Exact piece length: whole frames for video, else the range.
pub fn length(p: &Piece, fps: Option<f64>) -> f64 {
    match fps {
        Some(fps) if p.frames > 0 => p.frames as f64 / fps,
        _ => p.end - p.start,
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// Video quality options per encoder and preset (CQ/QP scales differ per encoder).
pub fn video_quality(encoder: &str, q: Quality, height: u32) -> Vec<String> {
    let high = q == Quality::High;
    match encoder {
        "h264_nvenc" => s(&["-preset", "p5", "-tune", "hq", "-rc", "vbr", "-cq", if high { "19" } else { "27" }, "-b:v", "0"]),
        "h264_amf" => {
            let (i, p) = if high { ("18", "20") } else { ("26", "28") };
            s(&["-quality", "quality", "-rc", "cqp", "-qp_i", i, "-qp_p", p])
        }
        "h264_qsv" => s(&["-preset", "slower", "-global_quality", if high { "20" } else { "28" }]),
        _ => {
            // Media Foundation: bitrate by resolution.
            let mbit = match (height >= 2000, height >= 1000, high) {
                (true, _, true) => 40,
                (true, _, false) => 14,
                (false, true, true) => 14,
                (false, true, false) => 5,
                (false, false, true) => 7,
                (false, false, false) => 3,
            };
            vec!["-b:v".into(), format!("{mbit}M")]
        }
    }
}

/// One intermediate file from up to [`GROUP`] pieces. `fps` is the output frame rate (constant).
pub fn group_args(src: &str, pieces: &[Piece], video: Option<(&str, Quality, u32, f64)>, out: &str) -> Vec<String> {
    let mut a = s(&["-hide_banner", "-v", "error", "-nostdin", "-y"]);
    for p in pieces {
        a.extend(s(&["-ss", &format!("{:.6}", p.start), "-t", &format!("{:.6}", p.end - p.start), "-i", src]));
    }
    let mut graph = String::new();
    let mut labels = String::new();
    for (i, p) in pieces.iter().enumerate() {
        let len = length(p, video.map(|v| v.3));
        if let Some((.., fps)) = video {
            // Constant rate, then exactly `frames` frames (the last one repeated if the source ends early).
            graph.push_str(&format!(
                "[{i}:v]setpts=PTS-STARTPTS,fps={fps},tpad=stop_mode=clone:stop_duration=1,trim=end_frame={},setpts=PTS-STARTPTS[v{i}];",
                p.frames
            ));
            labels.push_str(&format!("[v{i}]"));
        }
        let mut af = format!("[{i}:a]asetpts=PTS-STARTPTS,aresample=48000:async=1:first_pts=0");
        if p.fade_in > 0.0 {
            af.push_str(&format!(",afade=t=in:d={:.3}", p.fade_in));
        }
        if p.fade_out > 0.0 {
            af.push_str(&format!(",afade=t=out:st={:.6}:d={:.3}", (len - p.fade_out).max(0.0), p.fade_out));
        }
        // Pad/trim audio to exactly the piece length so every join stays in sync with the picture.
        af.push_str(&format!(",apad,atrim=0:{len:.6}[a{i}];"));
        graph.push_str(&af);
        labels.push_str(&format!("[a{i}]"));
    }
    let n = pieces.len();
    match video {
        Some((encoder, q, height, fps)) => {
            graph.push_str(&format!("{labels}concat=n={n}:v=1:a=1[cv][ca];[cv]format=yuv420p[v]"));
            a.extend(s(&["-filter_complex", &graph, "-map", "[v]", "-map", "[ca]", "-c:v", encoder]));
            a.extend(video_quality(encoder, q, height));
            // Closed GOPs, a keyframe at each file start, so stream copy can join the parts.
            a.extend(s(&["-g", &format!("{}", (fps * 2.0).round().max(1.0)), "-bf", "0"]));
        }
        None => {
            graph.push_str(&format!("{labels}concat=n={n}:v=0:a=1[ca]"));
            a.extend(s(&["-filter_complex", &graph, "-map", "[ca]"]));
        }
    }
    a.extend(s(&["-c:a", "pcm_s16le", "-ar", "48000", "-ac", "2", "-progress", "pipe:1", "-nostats", out]));
    a
}

/// Join the intermediate files (listed in `list`, concat-demuxer syntax): video copied, audio
/// encoded once to AAC.
pub fn final_args(list: &str, has_video: bool, out: &str) -> Vec<String> {
    let mut a = s(&["-hide_banner", "-v", "error", "-nostdin", "-y", "-f", "concat", "-safe", "0", "-i", list]);
    if has_video {
        a.extend(s(&["-c:v", "copy"]));
    }
    a.extend(s(&["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart", "-progress", "pipe:1", "-nostats", out]));
    a
}

/// Lines for the concat demuxer list file.
pub fn concat_list(files: &[String]) -> String {
    files.iter().map(|f| format!("file '{}'\n", f.replace('\\', "/").replace('\'', "'\\''"))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fades_only_at_joins() {
        let p = pieces(&[(0.0, 2.0), (3.0, 5.0), (6.0, 10.0)], 10.0, None, |t| t < 4.0);
        assert_eq!((p[0].fade_in, p[0].fade_out), (0.0, 0.010)); // file start, quiet cut
        assert_eq!((p[1].fade_in, p[1].fade_out), (0.010, 0.025)); // cut at 5 s is in sound
        assert_eq!((p[2].fade_in, p[2].fade_out), (0.025, 0.0)); // file end
    }

    #[test]
    fn group_reads_each_piece_with_its_own_seek() {
        // 6.25 s is not on the 30 fps grid: frame 187.5 rounds to 188 (6.2667 s); 4.0-6.2667 = 68 frames.
        let p = pieces(&[(1.5, 2.0), (4.0, 6.25)], 10.0, Some(30.0), |_| true);
        assert_eq!((p[0].frames, p[1].frames), (15, 68));
        let a = group_args("in.mp4", &p, Some(("h264_nvenc", Quality::High, 1080, 30.0)), "part.mkv").join(" ");
        assert!(a.contains("-ss 1.500000 -t 0.500000 -i in.mp4") && a.contains("-ss 4.000000 -t 2.266667 -i in.mp4"), "{a}");
        assert!(a.contains("trim=end_frame=68") && a.contains("concat=n=2:v=1:a=1") && a.contains("-cq 19"));
        // Sound is cut to exactly the same 68/30 s as the picture.
        assert!(a.contains("apad,atrim=0:2.266667") && a.contains("-c:a pcm_s16le") && a.ends_with("part.mkv"));
    }

    #[test]
    fn pieces_snap_to_frames_and_drop_empty_ones() {
        let p = pieces(&[(0.0, 0.01), (1.004, 2.0)], 3.0, Some(25.0), |_| true);
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].start, p[0].frames), (1.0, 25));
    }

    #[test]
    fn audio_only_groups_have_no_video_chain() {
        let p = pieces(&[(0.0, 1.0)], 1.0, None, |_| true);
        let a = group_args("in.wav", &p, None, "part.mkv").join(" ");
        assert!(a.contains("concat=n=1:v=0:a=1") && !a.contains("[0:v]"));
    }

    #[test]
    fn final_copies_video_and_encodes_audio_once() {
        let a = final_args("list.txt", true, "out.mp4").join(" ");
        assert!(a.contains("-f concat -safe 0 -i list.txt -c:v copy -c:a aac"));
        assert_eq!(concat_list(&["C:\\t\\it's.mkv".into()]), "file 'C:/t/it'\\''s.mkv'\n");
    }
}
