//! Ordered ONNX partitions with verified bindings and bounded intermediate lifetimes.
use anyhow::{ensure, Context, Result};
use ort::{
    session::Session,
    value::{DynValue, Tensor},
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Instant,
};

#[derive(Deserialize)]
struct Manifest {
    schema: u32,
    source_sha256: String,
    parts: Vec<Part>,
    #[serde(default)]
    optimization: Option<String>,
}
#[derive(Deserialize)]
struct Part {
    file: String,
    sha256: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
}
struct Stage {
    session: Session,
    inputs: Vec<String>,
    outputs: Vec<String>,
    keep: HashSet<String>,
}
pub struct Encoder {
    stages: Vec<Stage>,
}

impl Encoder {
    pub fn stage_count(&self) -> usize {
        self.stages.len()
    }
    pub fn single(session: Session) -> Self {
        Self {
            stages: vec![Stage {
                session,
                inputs: vec!["audio_signal".into(), "length".into()],
                outputs: vec!["outputs".into(), "encoded_lengths".into()],
                keep: HashSet::from(["outputs".into(), "encoded_lengths".into()]),
            }],
        }
    }
    pub fn load(path: &Path, threads: usize, flush: bool, prepack: bool) -> Result<Self> {
        let manifest: Manifest = serde_json::from_str(&std::fs::read_to_string(path)?)?;
        ensure!(
            manifest.schema == 1
                && manifest.source_sha256
                    == "8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2",
            "Invalid partition provenance"
        );
        ensure!(
            !manifest.parts.is_empty() && manifest.parts.len() <= 24,
            "Invalid partition count"
        );
        let mut stages = Vec::new();
        let mut available = HashSet::from(["audio_signal".to_string(), "length".to_string()]);
        for (i, part) in manifest.parts.iter().enumerate() {
            ensure!(
                Path::new(&part.file).components().count() == 1
                    && !Path::new(&part.file).is_absolute(),
                "Partition must be a basename"
            );
            let file = path
                .parent()
                .context("Manifest directory")?
                .join(&part.file);
            ensure!(
                crate::hash(&file)? == part.sha256,
                "Partition checksum mismatch: {}",
                part.file
            );
            ensure!(
                part.inputs.iter().all(|x| available.contains(x)),
                "Unresolved partition input"
            );
            let start = Instant::now();
            let session = crate::model::session(
                &file,
                threads,
                flush,
                prepack,
                false,
                false,
                manifest.optimization.as_deref() == Some("disabled"),
            )?;
            ensure!(
                session
                    .inputs()
                    .iter()
                    .map(|x| x.name())
                    .eq(part.inputs.iter().map(String::as_str))
                    && session
                        .outputs()
                        .iter()
                        .map(|x| x.name())
                        .eq(part.outputs.iter().map(String::as_str)),
                "Partition bindings mismatch"
            );
            eprintln!(
                "{} loaded in {:.3}s",
                part.file,
                start.elapsed().as_secs_f64()
            );
            available.extend(part.outputs.iter().cloned());
            let mut keep: HashSet<_> = manifest.parts[i + 1..]
                .iter()
                .flat_map(|p| p.inputs.iter().cloned())
                .collect();
            keep.extend(["outputs".into(), "encoded_lengths".into()]);
            stages.push(Stage {
                session,
                inputs: part.inputs.clone(),
                outputs: part.outputs.clone(),
                keep,
            });
        }
        ensure!(
            available.contains("outputs") && available.contains("encoded_lengths"),
            "Missing encoder outputs"
        );
        Ok(Self { stages })
    }
    pub fn run(
        &mut self,
        features: Vec<f32>,
        frames: usize,
        valid: usize,
        check: &impl Fn() -> Result<()>,
    ) -> Result<HashMap<String, DynValue>> {
        let mut values = HashMap::from([
            (
                "audio_signal".into(),
                Tensor::from_array(([1, 128, frames], features))?.into_dyn(),
            ),
            (
                "length".into(),
                Tensor::from_array(([1], vec![valid as i64]))?.into_dyn(),
            ),
        ]);
        for stage in &mut self.stages {
            check()?;
            let inputs: Vec<_> = stage
                .inputs
                .iter()
                .map(|name| {
                    Ok((
                        name.as_str(),
                        values.get(name).context("Missing encoder input")?,
                    ))
                })
                .collect::<Result<_>>()?;
            let mut outputs = stage.session.run(inputs)?;
            for name in &stage.outputs {
                values.insert(
                    name.clone(),
                    outputs.remove(name).context("Missing stage output")?,
                );
            }
            values.retain(|name, _| stage.keep.contains(name));
        }
        Ok(values)
    }
}
