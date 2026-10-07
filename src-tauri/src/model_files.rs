//! Manual model install: when the app cannot reach the download servers (blocked network, HTTPS
//! inspection), the user downloads the files in a browser and hands them to the app, which puts
//! each in place under the right name and checks it like a normal download.
use crate::models::ModelPaths;
use anyhow::{anyhow, ensure, Context, Result};
use std::path::{Path, PathBuf};

/// One file a model needs.
pub struct Source {
    pub url: String,
    pub dest: PathBuf,
    /// SHA-256 when the file is pinned; files without one are accepted by name.
    pub sha256: Option<&'static str>,
}

/// `kind`: "speech" (Whisper or Pianissimo), "text" (summary model) or "wordalign".
pub fn sources(paths: &ModelPaths, kind: &str, id: &str) -> Result<Vec<Source>> {
    Ok(match kind {
        "speech" if id == avskrift_pianissimo::ID => avskrift_pianissimo::FILES
            .iter()
            .map(|(file, sha)| Source {
                url: format!("https://huggingface.co/{}/resolve/{}/{file}", avskrift_pianissimo::REPO, avskrift_pianissimo::REVISION),
                dest: paths.pianissimo_dir.join(file),
                sha256: Some(*sha),
            })
            .collect(),
        "speech" => vec![Source {
            url: crate::models::whisper_url(id).ok_or_else(|| anyhow!("okänd modell: {id}"))?.to_string(),
            dest: paths.speech_file(id),
            sha256: None,
        }],
        "text" => {
            let (gguf, tok) = crate::models::summary_urls(id).ok_or_else(|| anyhow!("okänd modell: {id}"))?;
            let (gguf_dest, tok_dest) = paths.summary_files(id);
            vec![
                Source { url: gguf.to_string(), dest: gguf_dest, sha256: None },
                Source { url: tok.to_string(), dest: tok_dest, sha256: None },
            ]
        }
        "wordalign" => {
            let base = crate::wordalign::SOURCE.ok_or_else(|| anyhow!("Modellen för exakta ordtider är inte publicerad ännu."))?;
            avskrift_wordalign::FILES
                .iter()
                .map(|(file, sha)| Source { url: format!("{base}/{file}"), dest: paths.wordalign_dir.join(file), sha256: Some(*sha) })
                .collect()
        }
        _ => anyhow::bail!("okänd modelltyp: {kind}"),
    })
}

/// Name the browser saves the file under (the last part of the URL).
pub fn file_name(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

/// Undo a browser's "name (1).ext" for a repeated download.
fn plain_name(name: &str) -> String {
    let (stem, ext) = name.rsplit_once('.').map_or((name, ""), |(s, e)| (s, e));
    let stem = match stem.rsplit_once(" (") {
        Some((base, n)) if n.ends_with(')') && n[..n.len() - 1].chars().all(|c| c.is_ascii_digit()) => base,
        _ => stem,
    };
    if ext.is_empty() { stem.to_lowercase() } else { format!("{stem}.{ext}").to_lowercase() }
}

/// Which source a picked file is: by its name, else by a file extension only one source has.
fn matching<'a>(picked: &Path, sources: &'a [Source]) -> Option<&'a Source> {
    let name = plain_name(&picked.file_name()?.to_string_lossy());
    sources.iter().find(|s| file_name(&s.url).to_lowercase() == name).or_else(|| {
        let ext = Path::new(&name).extension()?.to_string_lossy().to_lowercase();
        let mut same = sources.iter().filter(|s| Path::new(file_name(&s.url)).extension().is_some_and(|e| e.to_string_lossy().to_lowercase() == ext));
        let only = same.next()?;
        same.next().is_none().then_some(only)
    })
}

/// Copy the picked files into place. Returns the URLs of files still missing.
pub fn import(paths: &ModelPaths, kind: &str, id: &str, picked: &[PathBuf]) -> Result<Vec<String>> {
    let sources = sources(paths, kind, id)?;
    for file in picked {
        let source = matching(file, &sources).ok_or_else(|| {
            anyhow!("{} hör inte till den här modellen. Välj filerna från länkarna ovan.", file.display())
        })?;
        if let Some(expected) = source.sha256 {
            ensure!(
                avskrift_pianissimo::hash(file)? == expected,
                "{} är inte rätt fil eller blev inte färdighämtad (kontrollsumman stämmer inte). Hämta den igen.",
                file.display()
            );
        }
        if let Some(parent) = source.dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let part = source.dest.with_extension("import");
        std::fs::copy(file, &part).with_context(|| format!("kunde inte kopiera {}", file.display()))?;
        std::fs::rename(&part, &source.dest).with_context(|| format!("kunde inte lägga {} på plats", source.dest.display()))?;
    }
    let missing: Vec<String> = sources.iter().filter(|s| !s.dest.is_file()).map(|s| s.url.clone()).collect();
    if missing.is_empty() && kind == "speech" && id == avskrift_pianissimo::ID {
        avskrift_pianissimo::mark_verified(&paths.pianissimo_dir)?;
    }
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn src(url: &str) -> Source {
        Source { url: url.into(), dest: PathBuf::from("x"), sha256: None }
    }
    #[test]
    fn browser_names_find_their_source() {
        let s = [src("https://h/a/Qwen2.5-3B-Instruct-Q8_0.gguf"), src("https://h/a/tokenizer.json")];
        assert_eq!(file_name(&matching(Path::new("C:/Downloads/tokenizer (1).json"), &s).unwrap().url), "tokenizer.json");
        assert_eq!(file_name(&matching(Path::new("C:/Downloads/Qwen2.5-3B-Instruct-Q8_0.gguf"), &s).unwrap().url), "Qwen2.5-3B-Instruct-Q8_0.gguf");
        // Renamed, but the only .gguf: still the model.
        assert!(matching(Path::new("C:/Downloads/modell.gguf"), &s).is_some());
        assert!(matching(Path::new("C:/Downloads/annat.txt"), &s).is_none());
        let two = [src("https://h/encoder-model.int8.onnx"), src("https://h/decoder_joint-model.int8.onnx")];
        assert!(matching(Path::new("C:/Downloads/okänd.onnx"), &two).is_none(), "ambiguous extension");
    }
}
