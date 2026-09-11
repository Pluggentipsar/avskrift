//! Orchestrates the full pipeline: detection (model + rules + dictionary) -> review data ->
//! pseudonymized export. Holds the lazily-loaded model and the most recent analysis.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serde::{Serialize, Deserialize};

use crate::ai::LlmDetector;
use crate::docio::{self, Format};
use crate::pii::dictionary::Dictionary;
use crate::pii::gazetteer;
use crate::pii::merge;
use crate::pii::model::NerModel;
use crate::pii::pseudonym::Pseudonymizer;
use crate::pii::{rules, Category, Source, Span};

const MODEL_CATEGORIES: [Category; 5] =
    [Category::Person, Category::Plats, Category::Organisation, Category::Tid, Category::Handelse];

pub struct ModelPaths {
    pub model: PathBuf,
    pub tokenizer: PathBuf,
    pub labels: PathBuf,
    pub llm_model: PathBuf,
    pub llm_tokenizer: PathBuf,
}

struct Analysis {
    text: String,
    spans: Vec<Span>,
    para_ranges: Vec<(usize, usize)>,
    /// Read only by `suggested_output_name`, which is not wired to a command yet.
    #[allow(dead_code)]
    source_path: Option<PathBuf>,
}

pub struct Engine {
    model: Mutex<crate::memory::Cache<NerModel>>,
    llm: Mutex<Option<LlmDetector>>,
    paths: ModelPaths,
    last: Mutex<Option<Analysis>>,
}

/// One piece of the document for rendering: plain run of text, optionally part of span `span`.
#[derive(Serialize)]
pub struct Segment {
    pub text: String,
    pub span: Option<usize>,
    /// Byte range of this segment in the analysed text (used to add manual masks by clicking).
    pub start: usize,
    pub end: usize,
    /// True for a clickable plain word (manual-mask target); false for detected spans / whitespace.
    pub word: bool,
    /// Index of the paragraph/utterance this segment belongs to (lets the UI show speaker labels).
    pub para: usize,
}

#[derive(Serialize)]
pub struct SpanInfo {
    pub id: usize,
    pub category: Category,
    pub source: Source,
    pub text: String,
    pub replacement: String,
}

#[derive(Serialize)]
pub struct AnalyzeResult {
    pub text: String,
    pub segments: Vec<Segment>,
    pub spans: Vec<SpanInfo>,
    pub counts: HashMap<String, usize>,
    pub warnings: Vec<String>,
    pub snapshot: ReviewSnapshot,
}

/// Offsets belong to this immutable UTF-8 text, never to the next model run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all="camelCase")]
pub struct ReviewSnapshot {
    pub text: String,
    pub spans: Vec<Span>,
    pub para_ranges: Vec<(usize,usize)>,
}

impl ReviewSnapshot {
    pub(crate) fn validate(&self, expected_text: &str) -> Result<()> {
        if self.text != expected_text { anyhow::bail!("granskningen hör till en annan textversion; originalet är kvar"); }
        let valid=|start:usize,end:usize| start <= end && end <= self.text.len()
            && self.text.is_char_boundary(start) && self.text.is_char_boundary(end);
        let mut end=0;
        for span in &self.spans {
            if !valid(span.start,span.end) || span.start == span.end || span.start < end
                || self.text.get(span.start..span.end) != Some(span.text.as_str()) {
                anyhow::bail!("ogiltiga maskeringar i den sparade versionen");
            }
            end=span.end;
        }
        end=0;
        for &(start,next) in &self.para_ranges {
            if !valid(start,next) || start < end { anyhow::bail!("ogiltiga stycken i den sparade versionen"); }
            end=next;
        }
        if self.para_ranges.is_empty() { anyhow::bail!("granskningen saknar stycken"); }
        Ok(())
    }
}

#[cfg(test)]
mod saved_review_tests {
    use super::*;
    #[test]
    fn restores_custom_swedish_mask_without_loading_a_model_and_rejects_bad_offsets() {
        let text="Åsa bor här";
        let snapshot=ReviewSnapshot{text:text.into(),spans:vec![Span::manual(0,4,"Åsa",Category::Person,Some("Eleven".into()))],para_ranges:vec![(0,text.len())]};
        let engine=Engine::new(ModelPaths{model:"missing".into(),tokenizer:"missing".into(),labels:"missing".into(),llm_model:"missing".into(),llm_tokenizer:"missing".into()});
        let serialized=serde_json::to_vec(&snapshot).unwrap();
        let restored=engine.restore_review(serde_json::from_slice(&serialized).unwrap(),text.into()).unwrap();
        assert_eq!(restored.spans[0].replacement,"Eleven");
        assert_eq!(engine.anonymized_text(vec![]).unwrap(),"Eleven bor här");
        let across=ReviewSnapshot{text:"Åsa\nbor här".into(),spans:vec![Span::manual(0,8,"Åsa\nbor",Category::Person,Some("Eleven".into()))],para_ranges:vec![(0,4),(5,13)]};
        engine.restore_review(across,"Åsa\nbor här".into()).unwrap();
        assert!(engine.anonymized_segments(vec![]).is_err());
        assert!(engine.add_manual_span(0,8,Category::Person,None).is_err());
        assert_eq!(engine.anonymized_text(vec![]).unwrap(),"Eleven här");
        engine.restore_review(snapshot.clone(),text.into()).unwrap();
        assert_eq!(engine.anonymized_text(vec![0]).unwrap(),text);
        assert!(engine.restore_review(snapshot.clone(),"Åsa flyttar".into()).is_err());
        let mut broken=snapshot.clone(); broken.spans[0].end=1;
        assert!(engine.restore_review(broken,text.into()).is_err());
        let mut overlap=snapshot;overlap.spans.push(overlap.spans[0].clone());
        assert!(engine.restore_review(overlap,text.into()).is_err());
        assert_eq!(engine.anonymized_text(vec![]).unwrap(),"Eleven bor här");
    }
}

impl Engine {
    pub fn restore_review(&self, snapshot: ReviewSnapshot, expected_text: String) -> Result<AnalyzeResult> {
        snapshot.validate(&expected_text)?;
        let result=build_result(&snapshot.text,&snapshot.spans,Vec::new(),&snapshot.para_ranges);
        *self.last.lock().unwrap()=Some(Analysis{text:snapshot.text,spans:snapshot.spans,para_ranges:snapshot.para_ranges,source_path:None});
        Ok(result)
    }
    pub fn new(paths: ModelPaths) -> Self {
        Engine { model: Mutex::new(crate::memory::Cache::new()), llm: Mutex::new(None), paths, last: Mutex::new(None) }
    }

    pub(crate) fn sweep(&self, now:std::time::Instant, pressure:bool) {
        if let Ok(mut model)=self.model.try_lock() { model.sweep(now,pressure); }
    }

    fn ensure_llm(&self) -> Result<()> {
        let mut guard = self.llm.lock().unwrap();
        if guard.is_none() {
            *guard = Some(LlmDetector::load(&self.paths.llm_model, &self.paths.llm_tokenizer)?);
        }
        Ok(())
    }

    fn detect(
        &self,
        text: &str,
        enabled: &HashSet<Category>,
        terms: &[String],
        use_ai: bool,
        progress: &dyn Fn(&str),
    ) -> Result<Vec<Span>> {
        crate::work::check()?;
        let mut spans: Vec<Span> = Vec::new();

        if MODEL_CATEGORIES.iter().any(|c| enabled.contains(c)) {
            progress("Förbereder NER-modell…");
            let mut guard = self.model.lock().unwrap();
            if guard.value.is_none() {
                guard.value = Some(NerModel::load(&self.paths.model, &self.paths.tokenizer, &self.paths.labels)?);
            }
            progress("Analyserar text…");
            let detected = guard.value.as_ref().unwrap().detect(text);
            guard.touch();
            spans.extend(detected?);
        }

        crate::work::check()?;
        spans.extend(rules::all(text));
        spans.extend(gazetteer::diagnoser(text));
        spans.extend(gazetteer::mediciner(text));
        spans.extend(gazetteer::namn(text));
        spans.extend(gazetteer::platser(text));
        spans.extend(Dictionary::new(terms, Category::Egen, true).detect(text));

        crate::work::check()?;
        if use_ai {
            if !self.paths.llm_model.exists() {
                return Err(anyhow!(
                    "Ladda ner sammanfattningsmodellen \"Liten (1,5B)\" i Sammanfatta-vyn först – \
                     den används även för djupare granskning (AI)."
                ));
            }
            progress("Djupare granskning (AI) – kan ta ~30 s…");
            self.ensure_llm()?;
            let guard = self.llm.lock().unwrap();
            if let Some(llm) = guard.as_ref() {
                let proposals = llm.propose(text)?;
                spans.extend(ai_spans(text, &proposals));
            }
        }

        Ok(merge::resolve(spans, enabled))
    }

    pub fn analyze_text(
        &self,
        text: String,
        enabled: Vec<Category>,
        terms: Vec<String>,
        use_ai: bool,
        progress: &dyn Fn(&str),
    ) -> Result<AnalyzeResult> {
        let enabled: HashSet<Category> = enabled.into_iter().collect();
        let spans = self.detect(&text, &enabled, &terms, use_ai, progress)?;
        let para_ranges = vec![(0, text.len())];
        let result = build_result(&text, &spans, Vec::new(), &para_ranges);
        crate::work::commit(|| *self.last.lock().unwrap() = Some(Analysis { text, spans, para_ranges, source_path: None }))?;
        Ok(result)
    }

    pub fn analyze_file(
        &self,
        path: PathBuf,
        enabled: Vec<Category>,
        terms: Vec<String>,
        use_ai: bool,
        progress: &dyn Fn(&str),
    ) -> Result<AnalyzeResult> {
        let doc = docio::load(&path)?;
        let enabled: HashSet<Category> = enabled.into_iter().collect();
        let spans = self.detect(&doc.text, &enabled, &terms, use_ai, progress)?;

        let mut warnings = Vec::new();
        if doc.has_tables {
            warnings.push(
                "Text från tabeller ingår i läsordning. Tabellernas layout följer inte med vid export.".to_string(),
            );
        }

        let result = build_result(&doc.text, &spans, warnings, &doc.para_ranges);
        crate::work::commit(|| *self.last.lock().unwrap() =
            Some(Analysis { text: doc.text, spans, para_ranges: doc.para_ranges, source_path: Some(path) }))?;
        Ok(result)
    }

    /// Anonymise a transcript: each utterance is treated as its own paragraph. Byte ranges are
    /// recorded so consistent pseudonyms apply across the whole transcript and the masked text can
    /// later be rebuilt per utterance (for SRT/VTT/Word export that keeps timestamps & speakers).
    pub fn analyze_segments(
        &self,
        segments: Vec<String>,
        enabled: Vec<Category>,
        terms: Vec<String>,
        use_ai: bool,
        progress: &dyn Fn(&str),
    ) -> Result<AnalyzeResult> {
        let mut text = String::new();
        let mut para_ranges = Vec::with_capacity(segments.len());
        for (i, seg) in segments.iter().enumerate() {
            let start = text.len();
            text.push_str(seg);
            para_ranges.push((start, text.len()));
            if i + 1 < segments.len() {
                text.push('\n');
            }
        }
        let enabled: HashSet<Category> = enabled.into_iter().collect();
        let spans = self.detect(&text, &enabled, &terms, use_ai, progress)?;
        let result = build_result(&text, &spans, Vec::new(), &para_ranges);
        crate::work::commit(|| *self.last.lock().unwrap() = Some(Analysis { text, spans, para_ranges, source_path: None }))?;
        Ok(result)
    }

    /// Add a span the user created by clicking an (undetected) word, optionally with a free-text
    /// replacement. Overrides any overlapping detected span; returns the rebuilt result. Span ids
    /// are renumbered, so the caller should reset its rejected set.
    pub fn add_manual_span(
        &self,
        start: usize,
        end: usize,
        category: Category,
        custom: Option<String>,
    ) -> Result<AnalyzeResult> {
        let mut guard = self.last.lock().unwrap();
        let analysis = guard.as_mut().ok_or_else(|| anyhow!("det finns ingen analys"))?;
        if start >= end
            || end > analysis.text.len()
            || !analysis.text.is_char_boundary(start)
            || !analysis.text.is_char_boundary(end)
        {
            return Err(anyhow!("ogiltigt textintervall"));
        }
        let surface = analysis.text[start..end].to_string();
        if !analysis.para_ranges.iter().any(|&(ps,pe)| start >= ps && end <= pe) {
            return Err(anyhow!("Markera text inom ett stycke i taget."));
        }
        let custom = custom.filter(|c| !c.trim().is_empty());
        analysis.spans.retain(|s| !(start < s.end && s.start < end));
        analysis.spans.push(Span::manual(start, end, &surface, category, custom));
        analysis.spans.sort_by_key(|s| s.start);
        Ok(build_result(&analysis.text, &analysis.spans, Vec::new(), &analysis.para_ranges))
    }

    /// The masked text of each stored paragraph (utterance), skipping rejected spans, with
    /// pseudonyms kept consistent across the whole transcript.
    pub fn anonymized_segments(&self, rejected: Vec<usize>) -> Result<Vec<String>> {
        let guard = self.last.lock().unwrap();
        let analysis = guard.as_ref().ok_or_else(|| anyhow!("det finns ingen analys"))?;
        let rejected: HashSet<usize> = rejected.into_iter().collect();
        let accepted: Vec<Span> =
            analysis.spans.iter().enumerate().filter(|(i, _)| !rejected.contains(i)).map(|(_, s)| s.clone()).collect();
        let mut pseudo = Pseudonymizer::new();
        let mut out = Vec::with_capacity(analysis.para_ranges.len());
        ensure_paragraph_masks(&accepted, &analysis.para_ranges)?;
        for &(ps, pe) in &analysis.para_ranges {
            let local: Vec<Span> = accepted
                .iter()
                .filter(|s| s.start >= ps && s.end <= pe)
                .map(|s| {
                    let mut c = s.clone();
                    c.start -= ps;
                    c.end -= ps;
                    c
                })
                .collect();
            let res = merge::apply(&analysis.text[ps..pe], &local, &mut pseudo);
            out.push(res.text);
        }
        Ok(out)
    }

    /// Apply pseudonyms (skipping rejected span ids) and write to `out_path`. Output format is
    /// chosen from the output file extension.
    pub fn export(&self, out_path: PathBuf, rejected: Vec<usize>) -> Result<()> {
        let guard = self.last.lock().unwrap();
        let analysis = guard.as_ref().ok_or_else(|| anyhow!("det finns ingen analys att exportera"))?;
        let rejected: HashSet<usize> = rejected.into_iter().collect();

        let accepted: Vec<Span> =
            analysis.spans.iter().enumerate().filter(|(i, _)| !rejected.contains(i)).map(|(_, s)| s.clone()).collect();

        let mut pseudo = Pseudonymizer::new();
        match Format::from_path(&out_path).unwrap_or(Format::Text) {
            Format::Text => {
                let res = merge::apply(&analysis.text, &accepted, &mut pseudo);
                docio::save_text(&out_path, &res.text)?;
            }
            Format::Docx => {
                ensure_paragraph_masks(&accepted, &analysis.para_ranges)?;
                let mut paragraphs = Vec::with_capacity(analysis.para_ranges.len());
                for &(ps, pe) in &analysis.para_ranges {
                    let local: Vec<Span> = accepted
                        .iter()
                        .filter(|s| s.start >= ps && s.end <= pe)
                        .map(|s| {
                            let mut c = s.clone();
                            c.start -= ps;
                            c.end -= ps;
                            c
                        })
                        .collect();
                    let res = merge::apply(&analysis.text[ps..pe], &local, &mut pseudo);
                    paragraphs.push(res.text);
                }
                docio::save_docx(&out_path, &paragraphs)?;
            }
        }
        Ok(())
    }

    /// The anonymized full text as a plain string (for copy-to-clipboard).
    pub fn anonymized_text(&self, rejected: Vec<usize>) -> Result<String> {
        let guard = self.last.lock().unwrap();
        let analysis = guard.as_ref().ok_or_else(|| anyhow!("det finns ingen analys"))?;
        let rejected: HashSet<usize> = rejected.into_iter().collect();
        let accepted: Vec<Span> =
            analysis.spans.iter().enumerate().filter(|(i, _)| !rejected.contains(i)).map(|(_, s)| s.clone()).collect();
        let mut pseudo = Pseudonymizer::new();
        Ok(merge::apply(&analysis.text, &accepted, &mut pseudo).text)
    }

    /// Default output path next to the source file, suffixed `_avidentifierad`.
    /// Not yet exposed as a command; kept for the export flow.
    #[allow(dead_code)]
    pub fn suggested_output_name(&self) -> Option<String> {
        let guard = self.last.lock().unwrap();
        let analysis = guard.as_ref()?;
        let path = analysis.source_path.as_ref()?;
        let stem = path.file_stem()?.to_string_lossy();
        let ext = path.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| "txt".into());
        Some(format!("{stem}_avidentifierad.{ext}"))
    }
}

fn ensure_paragraph_masks(spans: &[Span], ranges: &[(usize,usize)]) -> Result<()> {
    if spans.iter().any(|span| !ranges.iter().any(|&(start,end)| span.start >= start && span.end <= end)) {
        anyhow::bail!("En maskering går över en styckegräns. Dela maskeringen per stycke eller exportera hela den maskerade texten.");
    }
    Ok(())
}

fn build_result(text: &str, spans: &[Span], warnings: Vec<String>, para_ranges: &[(usize, usize)]) -> AnalyzeResult {
    let mut segments = build_segments(text, spans);
    // Tag each segment with its paragraph/utterance index so the UI can show speaker labels.
    for seg in &mut segments {
        seg.para = para_ranges.partition_point(|(s, _)| *s <= seg.start).saturating_sub(1);
    }

    let mut pseudo = Pseudonymizer::new();
    let mut counts: HashMap<String, usize> = HashMap::new();
    let span_info = spans
        .iter()
        .enumerate()
        .map(|(id, s)| {
            *counts.entry(category_key(s.category)).or_insert(0) += 1;
            SpanInfo {
                id,
                category: s.category,
                source: s.source,
                text: s.text.clone(),
                replacement: s.custom.clone().unwrap_or_else(|| pseudo.label_for(s.category, &s.text)),
            }
        })
        .collect();

    AnalyzeResult { text: text.to_string(), segments, spans: span_info, counts, warnings,
        snapshot: ReviewSnapshot{text:text.to_string(),spans:spans.to_vec(),para_ranges:para_ranges.to_vec()} }
}

fn build_segments(text: &str, spans: &[Span]) -> Vec<Segment> {
    let mut segs = Vec::new();
    let mut cursor = 0usize;
    for (i, s) in spans.iter().enumerate() {
        if s.start > cursor {
            push_plain(&text[cursor..s.start], cursor, &mut segs);
        }
        segs.push(Segment {
            text: text[s.start..s.end].to_string(),
            span: Some(i),
            start: s.start,
            end: s.end,
            word: false,
            para: 0,
        });
        cursor = s.end;
    }
    if cursor < text.len() {
        push_plain(&text[cursor..], cursor, &mut segs);
    }
    segs
}

/// Split a run of undetected text into word vs whitespace segments, each carrying its absolute byte
/// range, so the frontend can make individual words clickable for manual masking.
fn push_plain(run: &str, base: usize, segs: &mut Vec<Segment>) {
    let mut tok_start = 0usize;
    let mut cur_ws: Option<bool> = None;
    for (off, ch) in run.char_indices() {
        let ws = ch.is_whitespace();
        match cur_ws {
            None => cur_ws = Some(ws),
            Some(prev) if prev != ws => {
                segs.push(Segment {
                    text: run[tok_start..off].to_string(),
                    span: None,
                    start: base + tok_start,
                    end: base + off,
                    word: !prev,
                    para: 0,
                });
                tok_start = off;
                cur_ws = Some(ws);
            }
            _ => {}
        }
    }
    if let Some(prev) = cur_ws {
        if tok_start < run.len() {
            segs.push(Segment {
                text: run[tok_start..].to_string(),
                span: None,
                start: base + tok_start,
                end: base + run.len(),
                word: !prev,
                para: 0,
            });
        }
    }
}

fn category_key(c: Category) -> String {
    c.label().to_string()
}

/// Turn the LLM's verbatim substring suggestions into spans (every occurrence in the text).
fn ai_spans(text: &str, proposals: &[String]) -> Vec<Span> {
    let mut out = Vec::new();
    for p in proposals {
        let needle = p.trim();
        if needle.chars().count() < 2 {
            continue;
        }
        let mut from = 0usize;
        while let Some(rel) = text[from..].find(needle) {
            let s = from + rel;
            let e = s + needle.len();
            out.push(Span::new(s, e, needle, Category::Ovrigt, Source::Ai, 0.7));
            from = e;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paragraph_lookup_preserves_empty_paragraphs_and_separators() {
        let text = "Åsa\n\nErik\nHej";
        let ranges = [(0, 4), (5, 5), (6, 10), (11, 14)];
        let result = build_result(text, &[], vec![], &ranges);
        for seg in result.segments {
            let expected = ranges.iter().rposition(|(start, _)| seg.start >= *start).unwrap_or(0);
            assert_eq!(seg.para, expected);
        }
    }

    fn test_engine() -> Engine {
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/model");
        let llm = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/llm");
        Engine::new(ModelPaths {
            model: base.join("model.onnx"),
            tokenizer: base.join("tokenizer.json"),
            labels: base.join("labels.json"),
            llm_model: llm.join("model.gguf"),
            llm_tokenizer: llm.join("tokenizer.json"),
        })
    }

    /// The Url category masks a web address end-to-end without the NER model: enabling only
    /// `Url` skips the model (it is not a `MODEL_CATEGORIES` member), so the rule detector and
    /// pseudonymiser must do all the work. Guards the frontend "Webbadress" wiring at the seam.
    #[test]
    fn url_is_masked_end_to_end() {
        let engine = test_engine();
        let text = "Mer info på https://www.skolverket.se/sida och www.exempel.se.".to_string();
        let res = engine.analyze_text(text, vec![Category::Url], Vec::new(), false, &|_: &str| {}).unwrap();
        let urls = res.spans.iter().filter(|s| s.category == Category::Url).count();
        assert_eq!(urls, 2, "båda webbadresserna ska hittas");
        assert!(res.spans.iter().all(|s| s.replacement.starts_with("Webbadress")));

        let out = std::env::temp_dir().join("avident_url_test.txt");
        engine.export(out.clone(), Vec::new()).unwrap();
        let written = std::fs::read_to_string(&out).unwrap();
        assert!(!written.contains("skolverket.se"), "URL läckte: {written}");
        assert!(!written.contains("exempel.se"), "URL läckte: {written}");
        assert!(written.contains("Webbadress 1") && written.contains("Webbadress 2"));
    }

    /// Full text pipeline: detect -> export -> verify PII is gone. Run with:
    /// `cargo test --lib engine::tests -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn end_to_end_text() {
        let engine = test_engine();
        let text = "Anna Svensson har personnummer 811228-9874 och mejlar anna@example.se.".to_string();
        let res = engine.analyze_text(text, Category::ALL.to_vec(), Vec::new(), false, &|_: &str| {}).unwrap();
        assert!(res.spans.iter().any(|s| s.category == Category::Person));
        assert!(res.spans.iter().any(|s| s.category == Category::Personnummer));
        assert!(res.spans.iter().any(|s| s.category == Category::Epost));

        let out = std::env::temp_dir().join("avident_test_out.txt");
        engine.export(out.clone(), Vec::new()).unwrap();
        let written = std::fs::read_to_string(&out).unwrap();
        println!("TXT OUT: {written}");
        assert!(!written.contains("811228-9874"));
        assert!(!written.contains("anna@example.se"));
        assert!(!written.contains("Anna Svensson"));
        assert!(written.contains("Personnummer 1"));
    }

    #[test]
    #[ignore]
    fn docx_round_trip() {
        let engine = test_engine();
        let dir = std::env::temp_dir();
        let input = dir.join("avident_in.docx");
        crate::docio::save_docx(
            &input,
            &["Anna Svensson bor i Lund.".to_string(), "Mejl: anna@example.se".to_string()],
        )
        .unwrap();

        let res = engine.analyze_file(input, Category::ALL.to_vec(), Vec::new(), false, &|_: &str| {}).unwrap();
        assert!(res.spans.iter().any(|s| s.category == Category::Person));

        let out = dir.join("avident_out.docx");
        engine.export(out.clone(), Vec::new()).unwrap();

        let loaded = crate::docio::load(&out).unwrap();
        println!("DOCX OUT: {}", loaded.text);
        assert!(!loaded.text.contains("Anna Svensson"));
        assert!(!loaded.text.contains("anna@example.se"));
    }

    /// Confirms the AI layer surfaces hits as Övrigt through the full pipeline. Run with:
    /// `cargo test --release --lib engine::tests::ai_layer_surfaces_ovrigt -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn ai_layer_surfaces_ovrigt() {
        let engine = test_engine();
        let text =
            std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/veckobrev.txt"))
                .unwrap();
        let res =
            engine.analyze_text(text, Category::ALL.to_vec(), Vec::new(), true, &|m: &str| println!(">> {m}")).unwrap();
        let ovrigt = res.spans.iter().filter(|s| s.category == Category::Ovrigt).count();
        println!("Totalt {} träffar, varav Övrigt(AI): {ovrigt}", res.spans.len());
        for s in &res.spans {
            println!("  [{:?}/{:?}] {}", s.category, s.source, s.text);
        }
        assert!(ovrigt > 0, "AI-lagret gav inga Övrigt-träffar genom pipelinen");
    }
}
