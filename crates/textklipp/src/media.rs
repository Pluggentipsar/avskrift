//! Media information from `ffprobe -print_format json -show_format -show_streams`.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
    pub duration: f64,
    pub size_bytes: u64,
    pub container: String,
    pub video: Option<VideoInfo>,
    pub audio: Option<AudioInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub codec: String,
    pub width: u32,
    pub height: u32,
    /// Nominal frame rate (r_frame_rate).
    pub fps: f64,
    /// True when the average frame rate differs from the nominal one (phones): cut points must
    /// then be computed on the proxy's constant-rate timeline.
    pub variable_rate: bool,
    /// Clockwise display rotation in degrees (0, 90, 180, 270).
    pub rotation: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioInfo {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
}

fn rate(s: Option<&str>) -> Option<f64> {
    let (n, d) = s?.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d > 0.0 && n > 0.0).then(|| n / d)
}

impl MediaInfo {
    pub fn from_ffprobe(json: &str) -> Result<Self> {
        let v: serde_json::Value = serde_json::from_str(json)?;
        let format = &v["format"];
        let streams = v["streams"].as_array().cloned().unwrap_or_default();
        let num = |x: &serde_json::Value| x.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| x.as_f64());
        // Cover art is a video stream too; skip attached pictures.
        let video = streams
            .iter()
            .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"].as_i64().unwrap_or(0) == 0)
            .map(|s| {
                let fps = rate(s["r_frame_rate"].as_str()).unwrap_or(0.0);
                let avg = rate(s["avg_frame_rate"].as_str()).unwrap_or(fps);
                let rotation = s["side_data_list"]
                    .as_array()
                    .and_then(|l| l.iter().find_map(|d| d["rotation"].as_i64()))
                    .or_else(|| s["tags"]["rotate"].as_str().and_then(|r| r.parse().ok()))
                    .unwrap_or(0);
                VideoInfo {
                    codec: s["codec_name"].as_str().unwrap_or("").into(),
                    width: s["width"].as_u64().unwrap_or(0) as u32,
                    height: s["height"].as_u64().unwrap_or(0) as u32,
                    fps,
                    variable_rate: fps > 0.0 && (avg - fps).abs() / fps > 0.01,
                    rotation: (rotation.rem_euclid(360)) as i32,
                }
            });
        let audio = streams.iter().find(|s| s["codec_type"] == "audio").map(|s| AudioInfo {
            codec: s["codec_name"].as_str().unwrap_or("").into(),
            sample_rate: num(&s["sample_rate"]).unwrap_or(0.0) as u32,
            channels: s["channels"].as_u64().unwrap_or(0) as u32,
        });
        let duration = num(&format["duration"]).ok_or_else(|| anyhow!("filen saknar längduppgift"))?;
        Ok(Self {
            duration,
            size_bytes: num(&format["size"]).unwrap_or(0.0) as u64,
            container: format["format_name"].as_str().unwrap_or("").into(),
            video,
            audio,
        })
    }

    /// Disk needed next to the original: 16 kHz mono PCM16 audio + 720p proxy (~2.5 Mbit/s).
    pub fn working_bytes(&self) -> u64 {
        let audio = self.duration * 32_000.0;
        let proxy = if self.video.is_some() { self.duration * 2_500_000.0 / 8.0 } else { 0.0 };
        (audio + proxy) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_camera_file() {
        let json = r#"{"streams":[
            {"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"r_frame_rate":"50/1","avg_frame_rate":"50/1","disposition":{"attached_pic":0}},
            {"codec_type":"audio","codec_name":"aac","sample_rate":"48000","channels":2}],
            "format":{"duration":"1945.780000","size":"1469669869","format_name":"mov,mp4,m4a,3gp,3g2,mj2"}}"#;
        let m = MediaInfo::from_ffprobe(json).unwrap();
        assert_eq!(m.duration, 1945.78);
        let v = m.video.as_ref().unwrap();
        assert_eq!((v.width, v.height, v.fps, v.variable_rate, v.rotation), (1920, 1080, 50.0, false, 0));
        assert_eq!(m.audio.as_ref().unwrap().sample_rate, 48000);
        assert!(m.working_bytes() > 600_000_000 && m.working_bytes() < 700_000_000);
    }

    #[test]
    fn detects_phone_vfr_rotation_and_skips_cover_art() {
        let json = r#"{"streams":[
            {"codec_type":"video","codec_name":"mjpeg","width":300,"height":300,"r_frame_rate":"90000/1","disposition":{"attached_pic":1}},
            {"codec_type":"video","codec_name":"hevc","width":1920,"height":1080,"r_frame_rate":"30/1","avg_frame_rate":"29560/1000",
             "side_data_list":[{"rotation":-90}],"disposition":{"attached_pic":0}}],
            "format":{"duration":"12.5","size":"1000","format_name":"mov"}}"#;
        let v = MediaInfo::from_ffprobe(json).unwrap().video.unwrap();
        assert_eq!((v.codec.as_str(), v.variable_rate, v.rotation), ("hevc", true, 270));
    }
}
