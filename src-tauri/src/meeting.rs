//! Channel-local preparation and coverage for meeting speech. No transcript text is logged.
use crate::transcript::Utterance;

#[derive(Default)]
pub struct LiveResult {
    pub utterances: Vec<Utterance>,
    pub failed: bool,
    pub mic_blocks: usize,
    pub mic_text: usize,
    pub system_blocks: usize,
    pub system_text: usize,
}
impl LiveResult {
    pub fn complete(&self) -> bool {
        !self.failed && (self.mic_blocks == 0 || self.mic_text > 0)
            && (self.system_blocks == 0 || self.system_text > 0)
    }
}

/// Activity gate using sustained frame energy, rather than a high peak threshold.
/// This measures audio, not speech. Silence is never amplified into recognition input.
pub fn has_audio(samples: &[f32], rate: u32) -> bool {
    let frame = (rate as usize / 50).max(1);
    samples.chunks(frame).filter(|s| {
        let power = s.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / s.len().max(1) as f64;
        power > 0.0003f64.powi(2)
    }).take(3).count() >= 3
}

/// Bounded gain for quiet microphones. The recording itself is never modified.
pub fn prepare_mic(samples: &mut [f32]) {
    if !has_audio(samples, 16000) { return; }
    let peak = samples.iter().fold(0f32, |m, x| m.max(x.abs()));
    let gain = (0.2 / peak.max(0.000001)).clamp(1.0, 20.0);
    for x in samples { *x *= gain; }
}

pub fn load_channel(path: &str, label: &str) -> anyhow::Result<Option<Vec<f32>>> {
    use anyhow::Context;
    if path.is_empty() { return Ok(None); }
    let audio = crate::audio::load(std::path::Path::new(path))
        .with_context(|| format!("Kunde inte läsa {label}. Originalet behålls."))?;
    Ok(Some(audio.samples))
}

/// Whisper may place its final timestamp in its padded input. Keep the text, but bound playback
/// positions to the real recording/chunk before adding its absolute timeline offset.
pub fn utterances(raw:Vec<crate::transcribe::RawSegment>, samples:usize, label:&str, offset:f64) -> Vec<Utterance> {
    let duration=samples as f64/16000.0;
    let time=|value:f64| if value.is_finite(){value.clamp(0.0,duration)}else{0.0};
    crate::align::without_speakers(raw).into_iter().map(|mut u|{
        u.start=time(u.start);u.end=time(u.end).max(u.start);
        for word in &mut u.words {word.start=time(word.start);word.end=time(word.end).max(word.start);word.start+=offset;word.end+=offset;}
        u.start+=offset;u.end+=offset;u.speaker=Some(label.into());u
    }).collect()
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn quiet_voice_survives_without_amplifying_silence_or_clicks() {
        let mut quiet:Vec<f32>=(0..16000).map(|i|0.004*(i as f32/12.0).sin()).collect();
        assert!(has_audio(&quiet,16000)); prepare_mic(&mut quiet);
        assert!(quiet.iter().any(|x|x.abs()>0.05));
        let mut silence=vec![0.0;16000];prepare_mic(&mut silence);assert!(!has_audio(&silence,16000));
        silence[100]=1.0;assert!(!has_audio(&silence,16000));
    }
    #[test] fn missing_or_failed_channel_never_counts_as_complete_live_text() {
        let mut result=LiveResult::default();result.mic_blocks=4;result.system_blocks=5;result.system_text=6;
        assert!(!result.complete());result.mic_text=2;assert!(result.complete());result.failed=true;assert!(!result.complete());
    }
    #[test] fn unreadable_channel_is_an_error_not_silent_success() {
        assert!(load_channel("", "mikrofonen").unwrap().is_none());
        assert!(load_channel("nonexistent-avskrift-recording.wav", "mikrofonen").is_err());
    }
    #[test] #[ignore]
    fn quiet_synthetic_speech_reaches_real_whisper() {
        let model=std::env::var("AVSKRIFT_BENCH_MODEL").expect("existing Whisper model");
        let fixture=std::env::var("AVSKRIFT_MEETING_TEST_AUDIO").expect("synthetic speech fixture");
        let mut samples=crate::audio::load(std::path::Path::new(&fixture)).unwrap().samples;
        let peak=samples.iter().fold(0f32,|m,x|m.max(x.abs()));assert!(peak>0.0);
        for x in &mut samples {*x *= 0.004/peak;}
        assert!(samples.iter().all(|x|x.abs()<0.015),"old live gate would discard this whole recording");
        assert!(has_audio(&samples,16000));prepare_mic(&mut samples);
        let result=crate::transcribe::Transcriber::new().transcribe("quiet-fixture",std::path::Path::new(&model),&samples,"en",true,false,&|_|{},|_|{}).unwrap();
        let text=result.iter().map(|s|s.text.as_str()).collect::<Vec<_>>().join(" ").to_lowercase();
        assert!(text.contains("friday")&&text.contains("room"),"missing known synthetic speech: {text}");
        let result=utterances(result,samples.len(),"Jag",0.0);
        assert!(result.iter().all(|s|s.start>=0.0&&s.end<=samples.len() as f64/16000.0));
        println!("PASS: synthetic speech below the old microphone gate is transcribed with the real model.");
    }
    #[test] fn padded_timestamps_keep_text_and_absolute_channel_offset() {
        let raw=vec![crate::transcribe::RawSegment{start:1.0,end:9.0,text:"Behåll hela repliken".into(),words:vec![crate::transcribe::Word{start:1.5,end:8.0,text:"repliken".into()}]}];
        let out=utterances(raw,32000,"Jag",6.0);
        assert_eq!(out[0].text,"Behåll hela repliken");assert_eq!(out[0].start,7.0);assert_eq!(out[0].end,8.0);assert_eq!(out[0].words[0].end,8.0);
    }
    #[test] fn late_recording_save_keeps_completed_transcript_notes_and_organization() {
        use crate::jobs;
        let dir=std::env::temp_dir().join(format!("avskrift-meeting-save-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let mut draft:jobs::Job=serde_json::from_value(serde_json::json!({"version":2,"id":"meeting","jobType":"meeting","title":"Planering","createdAt":"2026-09-12","updatedAt":"2026-09-12","transcriptionPending":true,"notes":"Första anteckningen","agenda":"Lokalbokning","decisions":[{"text":"Boka fredag"}]})).unwrap();
        jobs::save_keeping_category(&dir,draft.clone()).unwrap();
        jobs::edit(&dir,"meeting",|j|{
            j.transcription_pending=false;
            j.transcript=Some(serde_json::from_value(serde_json::json!({"language":"sv","model":"test","diarized":true,"utterances":[{"start":0,"end":2,"speaker":"Jag","text":"Boka på fredag"}]})).unwrap());
            j.mix_wav_path=Some("test-mix.wav".into());j.extra.insert("pinned".into(),true.into());
            Ok(())
        }).unwrap();
        draft.notes="Anteckning efter slutförandet".into();draft.title="Nytt namn".into();
        jobs::save_keeping_category(&dir,draft).unwrap();
        let saved=jobs::open(&dir,"meeting").unwrap();
        assert_eq!(saved.title,"Nytt namn");assert_eq!(saved.notes,"Anteckning efter slutförandet");
        assert!(!saved.transcription_pending);assert_eq!(saved.transcript.unwrap().utterances.len(),1);
        assert_eq!(saved.mix_wav_path.as_deref(),Some("test-mix.wav"));
        let found=jobs::search(&dir,"lokalbokning");assert_eq!(found.len(),1);assert!(found[0].pinned);
        assert_eq!(jobs::search(&dir,"boka fredag").len(),1);
        let action=serde_json::from_value(serde_json::json!({"id":"action-1","text":"Boka","source":{"quote":"Boka på fredag","start":0}})).unwrap();
        jobs::add_job_action(&dir,"meeting",action,"2026-09-12").unwrap();
        let edited=serde_json::from_value(serde_json::json!({"text":"Boka lokalen","done":true})).unwrap();
        jobs::set_job_action(&dir,"meeting",0,edited,"2026-09-12").unwrap();
        assert_eq!(jobs::open(&dir,"meeting").unwrap().actions[0].extra["source"]["quote"],"Boka på fredag");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
