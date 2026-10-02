//! Textklipp: text-based video editing. Pure logic without app or model dependencies:
//! media information, FFmpeg command lines and the edit list. See docs/TEXTKLIPP-PLAN.md.

pub mod edl;
pub mod ffmpeg;
pub mod media;
pub mod render;
pub mod subtitles;

pub use edl::{keep_ranges, EditList, Item};
pub use media::MediaInfo;
