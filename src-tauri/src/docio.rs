//! Reading and writing of supported document formats: plain text and Word (.docx).
//!
//! Documents are flattened to a single string for detection and review. Each paragraph's byte
//! range in that string is recorded so the anonymized output can be split back per paragraph.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use docx_rs::{read_docx, DocumentChild, Docx, Paragraph, Run};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Docx,
}

impl Format {
    pub fn from_path(path: &Path) -> Result<Format> {
        let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
        match ext.as_deref() {
            Some("txt") | Some("text") | Some("md") => Ok(Format::Text),
            Some("docx") => Ok(Format::Docx),
            other => Err(anyhow!("filformatet stöds inte: {other:?}")),
        }
    }
}

/// A loaded document, flattened for detection.
pub struct LoadedDoc {
    /// The detected source format. Recorded on load; not yet read by callers.
    #[allow(dead_code)]
    pub format: Format,
    /// Paragraphs joined by `\n`.
    pub text: String,
    /// Byte range of each paragraph within `text` (excluding the separator).
    pub para_ranges: Vec<(usize, usize)>,
    /// True if the source contained tables; their text is currently not processed (v1 limitation).
    pub has_tables: bool,
}

pub fn load(path: &Path) -> Result<LoadedDoc> {
    match Format::from_path(path)? {
        Format::Text => {
            let text = std::fs::read_to_string(path).with_context(|| format!("kunde inte läsa {}", path.display()))?;
            let range = (0, text.len());
            Ok(LoadedDoc { format: Format::Text, para_ranges: vec![range], text, has_tables: false })
        }
        Format::Docx => {
            let bytes = std::fs::read(path).with_context(|| format!("kunde inte läsa {}", path.display()))?;
            let docx = read_docx(&bytes).map_err(|e| anyhow!("kunde inte tolka Word-filen: {e:?}"))?;

            let mut text = String::new();
            let mut para_ranges = Vec::new();
            let mut has_tables = false;

            for child in &docx.document.children {
                match child {
                    DocumentChild::Paragraph(p) => {
                        let start = text.len();
                        text.push_str(&p.raw_text());
                        para_ranges.push((start, text.len()));
                        text.push('\n');
                    }
                    DocumentChild::Table(_) => has_tables = true,
                    _ => {}
                }
            }

            Ok(LoadedDoc { format: Format::Docx, text, para_ranges, has_tables })
        }
    }
}

pub fn save_text(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content).with_context(|| format!("kunde inte skriva {}", path.display()))
}

/// Write a .docx from anonymized paragraph texts. Rebuilds a clean document (paragraph structure
/// preserved; original inline styling is not carried over in v1).
pub fn save_docx(path: &Path, paragraphs: &[String]) -> Result<()> {
    let mut docx = Docx::new();
    for p in paragraphs {
        docx = docx.add_paragraph(Paragraph::new().add_run(Run::new().add_text(p.as_str())));
    }
    let file = std::fs::File::create(path).with_context(|| format!("kunde inte skapa {}", path.display()))?;
    docx.build().pack(file).map_err(|e| anyhow!("kunde inte skriva Word-filen: {e:?}"))?;
    Ok(())
}
