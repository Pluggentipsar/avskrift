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
/// Groups rendered at the same time (two FFmpeg processes ≈ twice as fast with NVENC).
pub const WORKERS: usize = 2;
/// Room tone: at joins where both sides are quiet, the pieces' audio overlaps by this much
/// (equal-power crossfade) instead of fading each side to digital silence. The room's own noise
/// then runs on without a dip; piece lengths, and therefore sync, are unchanged.
pub const XFADE: f64 = 0.030;

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
    /// The cut at the start / end lies in silence (false at the file's own start/end).
    pub quiet_in: bool,
    pub quiet_out: bool,
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
            let (first, last) = (a <= 0.001, b >= duration - 0.001);
            let (quiet_in, quiet_out) = (!first && quiet(a), !last && quiet(b));
            Some(Piece {
                start,
                end,
                frames,
                fade_in: if first { 0.0 } else { fade_for(quiet_in) },
                fade_out: if last { 0.0 } else { fade_for(quiet_out) },
                quiet_in,
                quiet_out,
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

/// For each piece in a group: does it crossfade (room tone) into the next one? Only at quiet
/// joins, and only between pieces long enough to carry the overlap.
pub fn blends(pieces: &[Piece], fps: Option<f64>) -> Vec<bool> {
    (0..pieces.len())
        .map(|i| {
            i + 1 < pieces.len()
                && pieces[i].quiet_out
                && pieces[i + 1].quiet_in
                && length(&pieces[i], fps) > 3.0 * XFADE
                && length(&pieces[i + 1], fps) > 3.0 * XFADE
        })
        .collect()
}

/// Split pieces into render groups of about [`GROUP`]. A group ends only where the join fades
/// anyway (not at a room-tone crossfade, which cannot span two intermediate files), unless the
/// group reaches twice the target size.
pub fn groups(pieces: &[Piece], fps: Option<f64>) -> Vec<std::ops::Range<usize>> {
    let blend = blends(pieces, fps);
    let mut out = Vec::new();
    let mut start = 0;
    for i in 0..pieces.len() {
        let size = i + 1 - start;
        let last = i + 1 == pieces.len();
        if last || (size >= GROUP && !blend[i]) || size >= 2 * GROUP {
            out.push(start..i + 1);
            start = i + 1;
        }
    }
    out
}

/// One intermediate file from up to [`GROUP`] pieces. `fps` is the output frame rate (constant).
pub fn group_args(src: &str, pieces: &[Piece], video: Option<(&str, Quality, u32, f64)>, out: &str) -> Vec<String> {
    let fps = video.map(|v| v.3);
    let blend = blends(pieces, fps);
    let mut a = s(&["-hide_banner", "-v", "error", "-nostdin", "-y"]);
    for (i, p) in pieces.iter().enumerate() {
        // A piece that crossfades into the next reads XFADE more audio; its picture is still cut to
        // exactly `frames` below, so only the sound overlaps.
        let extra = if blend[i] { XFADE } else { 0.0 };
        a.extend(s(&["-ss", &format!("{:.6}", p.start), "-t", &format!("{:.6}", p.end - p.start + extra), "-i", src]));
    }
    let mut chains: Vec<String> = Vec::new();
    let mut video_labels = String::new();
    for (i, p) in pieces.iter().enumerate() {
        let len = length(p, fps);
        if let Some(fps) = fps {
            // Constant rate, then exactly `frames` frames (the last one repeated if the source ends early).
            chains.push(format!(
                "[{i}:v]setpts=PTS-STARTPTS,fps={fps},tpad=stop_mode=clone:stop_duration=1,trim=end_frame={},setpts=PTS-STARTPTS[v{i}]",
                p.frames
            ));
            video_labels.push_str(&format!("[v{i}]"));
        }
        let blended_in = i > 0 && blend[i - 1];
        let extra = if blend[i] { XFADE } else { 0.0 };
        let mut af = format!("[{i}:a]asetpts=PTS-STARTPTS,aresample=48000:async=1:first_pts=0");
        if p.fade_in > 0.0 && !blended_in {
            af.push_str(&format!(",afade=t=in:d={:.3}", p.fade_in));
        }
        if p.fade_out > 0.0 && !blend[i] {
            af.push_str(&format!(",afade=t=out:st={:.6}:d={:.3}", (len - p.fade_out).max(0.0), p.fade_out));
        }
        // Pad/trim audio to exactly the piece length (+ overlap) so every join stays in sync.
        af.push_str(&format!(",apad,atrim=0:{:.6}[a{i}]", len + extra));
        chains.push(af);
    }
    // Sound: piece by piece, crossfaded where blended (the overlap is consumed, so the total
    // equals the sum of piece lengths), otherwise simply appended.
    let mut sound = "a0".to_string();
    for i in 1..pieces.len() {
        let next = format!("m{i}");
        chains.push(if blend[i - 1] {
            format!("[{sound}][a{i}]acrossfade=d={XFADE}:c1=qsin:c2=qsin[{next}]")
        } else {
            format!("[{sound}][a{i}]concat=n=2:v=0:a=1[{next}]")
        });
        sound = next;
    }
    let n = pieces.len();
    let sound_map = format!("[{sound}]");
    match video {
        Some((encoder, q, height, fps)) => {
            chains.push(format!("{video_labels}concat=n={n}:v=1:a=0[cv]"));
            chains.push("[cv]format=yuv420p[v]".into());
            a.extend(s(&["-filter_complex", &chains.join(";"), "-map", "[v]", "-map", &sound_map, "-c:v", encoder]));
            a.extend(video_quality(encoder, q, height));
            // Closed GOPs, a keyframe at each file start, so stream copy can join the parts.
            a.extend(s(&["-g", &format!("{}", (fps * 2.0).round().max(1.0)), "-bf", "0"]));
        }
        None => {
            a.extend(s(&["-filter_complex", &chains.join(";"), "-map", &sound_map]));
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
        // The join at 2.0/4.0 is in sound, so no room-tone overlap: plain lengths.
        let p = pieces(&[(1.5, 2.0), (4.0, 6.25)], 10.0, Some(30.0), |t| t < 1.6 || t > 6.0);
        assert_eq!((p[0].frames, p[1].frames), (15, 68));
        assert_eq!(blends(&p, Some(30.0)), [false, false]);
        let a = group_args("in.mp4", &p, Some(("h264_nvenc", Quality::High, 1080, 30.0)), "part.mkv").join(" ");
        assert!(a.contains("-ss 1.500000 -t 0.500000 -i in.mp4") && a.contains("-ss 4.000000 -t 2.266667 -i in.mp4"), "{a}");
        assert!(a.contains("trim=end_frame=68") && a.contains("concat=n=2:v=1:a=0") && a.contains("-cq 19"));
        assert!(a.contains("[a0][a1]concat=n=2:v=0:a=1[m1]") && a.contains("-map [m1]"));
        // Sound is cut to exactly the same 68/30 s as the picture, with fades at the loud join.
        assert!(a.contains("apad,atrim=0:2.266667") && a.contains("afade=t=out") && a.ends_with("part.mkv"));
    }

    #[test]
    fn quiet_joins_overlap_room_tone_instead_of_fading_to_silence() {
        let p = pieces(&[(1.5, 2.0), (4.0, 6.25)], 10.0, Some(30.0), |_| true);
        assert_eq!(blends(&p, Some(30.0)), [true, false]);
        let a = group_args("in.mp4", &p, Some(("h264_nvenc", Quality::High, 1080, 30.0)), "part.mkv").join(" ");
        // The first piece reads 30 ms more audio, keeps its picture at 15 frames, and crossfades.
        assert!(a.contains("-ss 1.500000 -t 0.530000 -i in.mp4") && a.contains("trim=end_frame=15"), "{a}");
        assert!(a.contains("apad,atrim=0:0.530000[a0]") && a.contains("[a0][a1]acrossfade=d=0.03:c1=qsin:c2=qsin[m1]"));
        // No fade to silence on either side of the blended join.
        let first = a.split("[a0]").next().unwrap();
        assert!(!first.contains("afade=t=out"));
    }

    #[test]
    fn groups_end_at_fading_joins_not_inside_room_tone() {
        // 60 one-second pieces; joins are quiet (room tone) except after piece 29.
        let keep: Vec<(f64, f64)> = (0..60).map(|i| (i as f64 * 2.0, i as f64 * 2.0 + 1.0)).collect();
        let p = pieces(&keep, 200.0, Some(25.0), |t| (t - 59.0).abs() > 0.01 && (t - 60.0).abs() > 0.01);
        let g = groups(&p, Some(25.0));
        // Not after 24 pieces (a crossfade there), but at the first fading join after it.
        assert_eq!(g[0], 0..30);
        assert_eq!(g.iter().map(|r| r.len()).sum::<usize>(), 60);
        // All quiet: forced split at twice the target.
        let p = pieces(&keep, 200.0, Some(25.0), |_| true);
        assert_eq!(groups(&p, Some(25.0))[0], 0..48);
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
        assert!(a.contains("-map [a0]") && !a.contains("[0:v]"));
    }

    #[test]
    fn final_copies_video_and_encodes_audio_once() {
        let a = final_args("list.txt", true, "out.mp4").join(" ");
        assert!(a.contains("-f concat -safe 0 -i list.txt -c:v copy -c:a aac"));
        assert_eq!(concat_list(&["C:\\t\\it's.mkv".into()]), "file 'C:/t/it'\\''s.mkv'\n");
    }
}
