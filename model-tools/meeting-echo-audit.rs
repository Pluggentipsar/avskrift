// Standalone reproduction against the actual production text-echo filter.
// rustc model-tools/meeting-echo-audit.rs -o .build-tools/meeting-echo-audit.exe
#![allow(dead_code)]
mod diarize { pub struct SpeakerTurn {pub start:f64,pub end:f64,pub speaker:usize} }
mod transcribe {
    pub struct Word {pub start:f64,pub end:f64,pub text:String}
    pub struct RawSegment {pub start:f64,pub end:f64,pub text:String,pub words:Vec<Word>}
}
mod transcript {
    pub struct Word {pub start:f64,pub end:f64,pub text:String}
    pub struct Utterance {pub start:f64,pub end:f64,pub speaker:Option<String>,pub text:String,pub words:Vec<Word>}
}
#[path="../src-tauri/src/align.rs"] mod align;
fn main() {
    let u=|start,end,speaker:&str,text:&str| transcript::Utterance {start,end,speaker:Some(speaker.into()),text:text.into(),words:vec![]};
    let out=align::from_labeled(vec![
        u(1.0,4.0,"Mötet","Vi ska inte boka lokalen nu."),
        u(4.2,6.0,"Jag","Vi ska boka lokalen nu."),
    ]);
    println!("Genuine contrary reply retained: {}",out.iter().any(|u|u.speaker.as_deref()==Some("Jag")));
    assert_eq!(out.len(),2);
    println!("PASS: both original replies are retained.");
}
