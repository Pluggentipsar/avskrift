//! One-time local optimization and partitioning. ONNX fields are copied byte for byte;
//! only the graph container and its input/output lists are rebuilt (no weight decoding).
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    io::Write,
    path::{Path, PathBuf},
};

fn identity() -> Value {
    json!({"version":1,"ort":ort::info(),"os":std::env::consts::OS,"arch":std::env::consts::ARCH,
        "cpu":std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_default(),"source":crate::FILES[0].1,"precision":true})
}
pub fn ready(dir: &Path) -> bool {
    std::fs::read(dir.join("prepared/manifest.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|m| {
            m["identity"] == identity()
                && m["schema"] == 1
                && m["source_sha256"] == crate::FILES[0].1
                && m["optimization"] == "disabled"
                && m["parts"].as_array().is_some_and(|p| {
                    p.len() == 12
                        && p.iter().all(|v| {
                            v["file"].as_str().is_some_and(|f| {
                                Path::new(f).components().count() == 1
                                    && dir.join("prepared").join(f).is_file()
                            })
                        })
                })
        })
}
pub fn prepare(
    dir: &Path,
    threads: usize,
    progress: &dyn Fn(&str),
    check: &dyn Fn() -> Result<()>,
) -> Result<PathBuf> {
    prepare_with_admission(dir, threads, progress, check, &|| Ok(()))
}

/// Admission is checked only for rebuilding, after an intact cache has been reused.
pub fn prepare_with_admission(
    dir: &Path,
    threads: usize,
    progress: &dyn Fn(&str),
    check: &dyn Fn() -> Result<()>,
    before_build: &dyn Fn() -> Result<()>,
) -> Result<PathBuf> {
    crate::verify(dir)?;
    let dest = dir.join("prepared");
    if ready(dir) {
        let m: Value = serde_json::from_slice(&std::fs::read(dest.join("manifest.json"))?)?;
        if m["parts"].as_array().unwrap().iter().all(|p| {
            p["file"]
                .as_str()
                .zip(p["sha256"].as_str())
                .is_some_and(|(f, h)| crate::hash(&dest.join(f)).ok().as_deref() == Some(h))
        }) {
            return Ok(dest.join("manifest.json"));
        }
    }
    check()?;
    before_build()?;
    // Only this owned staging directory is replaced; the ready bundle stays intact until success.
    let staging = dir.join("preparing-v1");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    progress("Förbereder Pianissimo på datorn. Detta görs en gång och kan ta flera minuter…");
    let optimized = staging.join("whole.onnx");
    let session = ort::session::Session::builder()?
        .with_intra_threads(threads)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_precise_qmm()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_optimization_level(ort::session::builder::GraphOptimizationLevel::Level3)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .with_optimized_model_path(&optimized)
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .commit_from_file(dir.join(crate::FILES[0].0))?;
    drop(session);
    check()?;
    progress("Färdigställer Pianissimo för snabbare modellstart…");
    split(&optimized, &staging, check)?;
    std::fs::remove_file(&optimized)?;
    check()?;
    if dest.exists() {
        std::fs::remove_dir_all(&dest)?;
    }
    std::fs::rename(staging, &dest)?;
    Ok(dest.join("manifest.json"))
}

#[derive(Clone, Copy)]
struct Field<'a> {
    tag: u32,
    raw: &'a [u8],
    data: &'a [u8],
}
fn varint(data: &[u8], at: &mut usize) -> Result<u64> {
    let mut n = 0;
    for shift in (0..70).step_by(7) {
        let b = *data.get(*at).context("Truncated ONNX varint")?;
        *at += 1;
        ensure!(shift < 63 || b <= 1, "ONNX varint overflow");
        n |= ((b & 127) as u64) << shift;
        if b < 128 {
            return Ok(n);
        }
    }
    bail!("Invalid ONNX varint")
}
fn fields(data: &[u8]) -> Result<Vec<Field<'_>>> {
    let mut at = 0;
    let mut out = Vec::new();
    while at < data.len() {
        let start = at;
        let key = varint(data, &mut at)?;
        ensure!(
            key >> 3 > 0 && key >> 3 <= u32::MAX as u64,
            "Invalid ONNX field"
        );
        let payload = match key & 7 {
            0 => {
                let start = at;
                varint(data, &mut at)?;
                &data[start..at]
            }
            wire @ (1 | 2 | 5) => {
                let len = match wire {
                    1 => 8,
                    5 => 4,
                    _ => usize::try_from(varint(data, &mut at)?)?,
                };
                let end = at.checked_add(len).context("ONNX field overflow")?;
                let bytes = data.get(at..end).context("Truncated ONNX field")?;
                at = end;
                bytes
            }
            _ => bail!("Unsupported ONNX wire type"),
        };
        out.push(Field {
            tag: (key >> 3) as u32,
            raw: &data[start..at],
            data: payload,
        });
    }
    Ok(out)
}
fn names<'a>(fs: &[Field<'a>], tag: u32) -> Result<Vec<&'a str>> {
    fs.iter()
        .filter(|f| f.tag == tag)
        .map(|f| Ok(std::str::from_utf8(f.data)?))
        .collect()
}
fn name<'a>(data: &'a [u8], tag: u32) -> Result<&'a str> {
    Ok(names(&fields(data)?, tag)?.first().copied().unwrap_or(""))
}
fn write_varint(out: &mut impl Write, mut n: u64) -> Result<()> {
    while n >= 128 {
        out.write_all(&[(n as u8) | 128])?;
        n >>= 7;
    }
    out.write_all(&[n as u8])?;
    Ok(())
}
fn header(out: &mut impl Write, tag: u32, len: usize) -> Result<()> {
    write_varint(out, ((tag << 3) | 2) as u64)?;
    write_varint(out, len as u64)
}
fn encoded_len(tag: u32, data: &[u8]) -> usize {
    let mut h = Vec::new();
    header(&mut h, tag, data.len()).unwrap();
    h.len() + data.len()
}
struct Node<'a> {
    raw: &'a [u8],
    inputs: Vec<&'a str>,
    outputs: Vec<&'a str>,
    stage: usize,
}

/// Public for reproducible development checks against the Python reference splitter.
pub fn split(input: &Path, out: &Path, check: &dyn Fn() -> Result<()>) -> Result<()> {
    let bytes = std::fs::read(input)?;
    let model = fields(&bytes)?;
    let graph = fields(
        model
            .iter()
            .find(|f| f.tag == 7)
            .context("Missing ONNX graph")?
            .data,
    )?;
    ensure!(
        !graph.iter().any(|f| f.tag == 15),
        "Sparse initializers are unsupported"
    );
    let mut info = BTreeMap::new();
    let mut weights = BTreeMap::new();
    let mut constants = BTreeMap::new();
    let mut nodes = Vec::new();
    let layers = regex::Regex::new(r"/layers\.(\d+)/")?;
    let mut final_names = BTreeSet::new();
    let mut available = BTreeSet::new();
    for f in &graph {
        match f.tag {
            11 | 12 | 13 => {
                let n = name(f.data, 1)?;
                info.insert(n, f.data);
                if f.tag == 11 {
                    available.insert(n);
                }
                if f.tag == 12 {
                    final_names.insert(n);
                }
            }
            5 => {
                weights.insert(name(f.data, 8)?, f.data);
            }
            1 => {
                let fs = fields(f.data)?;
                let outputs = names(&fs, 2)?;
                if name(f.data, 4)? == "Constant" {
                    ensure!(outputs.len() == 1, "Unexpected Constant outputs");
                    constants.insert(outputs[0], f.data);
                } else {
                    let stage = layers
                        .captures(name(f.data, 3)?)
                        .map(|c| c[1].parse::<usize>())
                        .transpose()?
                        .map(|n| n / 2)
                        .unwrap_or(11);
                    ensure!(stage < 12, "Unexpected encoder layer");
                    nodes.push(Node {
                        raw: f.data,
                        inputs: names(&fs, 1)?,
                        outputs,
                        stage,
                    });
                }
            }
            _ => {}
        }
    }
    let producer: HashMap<_, _> = nodes
        .iter()
        .enumerate()
        .flat_map(|(i, n)| {
            n.outputs
                .iter()
                .filter(|s| !s.is_empty())
                .map(move |s| (*s, i))
        })
        .collect();
    for i in (0..nodes.len()).rev() {
        let stage = nodes[i].stage;
        for input in nodes[i].inputs.clone() {
            if let Some(&p) = producer.get(input) {
                ensure!(p < i, "Encoder must be topologically ordered");
                nodes[p].stage = nodes[p].stage.min(stage);
            }
        }
    }
    let mut last_consumer = HashMap::<&str, usize>::new();
    for n in &nodes {
        for s in &n.inputs {
            last_consumer
                .entry(s)
                .and_modify(|v| *v = (*v).max(n.stage))
                .or_insert(n.stage);
        }
    }
    let mut parts = Vec::new();
    for stage in 0..12 {
        check()?;
        let group: Vec<_> = nodes.iter().filter(|n| n.stage == stage).collect();
        ensure!(!group.is_empty(), "Empty encoder partition");
        let produced: BTreeSet<_> = group
            .iter()
            .flat_map(|n| n.outputs.iter().copied())
            .filter(|s| !s.is_empty())
            .collect();
        let needed: BTreeSet<_> = group
            .iter()
            .flat_map(|n| n.inputs.iter().copied())
            .filter(|s| !s.is_empty())
            .collect();
        let external: Vec<_> = needed.difference(&produced).copied().collect();
        let inputs: Vec<_> = external
            .iter()
            .copied()
            .filter(|s| !weights.contains_key(s) && !constants.contains_key(s))
            .collect();
        ensure!(
            inputs.iter().all(|s| available.contains(s)),
            "Unresolved partition inputs"
        );
        let outputs: Vec<_> = produced
            .iter()
            .copied()
            .filter(|s| final_names.contains(s) || last_consumer.get(s).is_some_and(|i| *i > stage))
            .collect();
        available.extend(outputs.iter().copied());
        let mut payload: Vec<(u32, &[u8])> = Vec::new();
        let mut internal = needed.union(&produced).copied().collect::<BTreeSet<_>>();
        for s in &external {
            if let Some(c) = constants.get(s) {
                payload.push((1, c));
                for n in names(&fields(c)?, 2)? {
                    internal.insert(n);
                }
            }
        }
        for n in &group {
            payload.push((1, n.raw));
        }
        let graph_name = format!("pianissimo_encoder_{stage}");
        payload.push((2, graph_name.as_bytes()));
        for s in &external {
            if let Some(w) = weights.get(s) {
                payload.push((5, w));
            }
        }
        for (tag, names) in [(11, &inputs), (12, &outputs)] {
            for s in names {
                payload.push((
                    tag,
                    *info
                        .get(s)
                        .with_context(|| format!("Missing boundary type: {s}"))?,
                ));
                internal.remove(s);
            }
        }
        for (s, v) in &info {
            if internal.contains(s) {
                payload.push((13, v));
            }
        }
        let file = format!("encoder-{stage:02}.onnx");
        let mut writer = std::io::BufWriter::new(std::fs::File::create(out.join(&file))?);
        // Preserve all model-level metadata/opsets; graph bytes are streamed without copying weights.
        for f in &model {
            if f.tag != 7 {
                writer.write_all(f.raw)?;
            }
        }
        header(
            &mut writer,
            7,
            payload.iter().map(|(t, d)| encoded_len(*t, d)).sum(),
        )?;
        for (tag, data) in payload {
            header(&mut writer, tag, data.len())?;
            writer.write_all(data)?;
        }
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        parts.push(json!({"file":file,"sha256":crate::hash(&out.join(&file))?,"inputs":inputs,"outputs":outputs}));
    }
    ensure!(
        final_names.iter().all(|s| available.contains(s)),
        "Missing encoder outputs"
    );
    std::fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "schema":1,"source_sha256":crate::FILES[0].1,"optimization":"disabled","parts":parts,"identity":identity()
        }))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_truncated_and_overflowing_fields() {
        assert!(fields(&[0x0a, 5, 1]).is_err());
        assert!(fields(&[0x0a, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 2]).is_err());
        assert!(fields(&[0]).is_err());
    }
    #[test]
    fn copies_unknown_fields_exactly() {
        let raw = [0x08, 0x96, 0x01, 0x12, 0x02, b'h', b'i'];
        let fs = fields(&raw).unwrap();
        assert_eq!(
            fs.iter()
                .flat_map(|f| f.raw.iter().copied())
                .collect::<Vec<_>>(),
            raw
        );
        assert_eq!(names(&fs, 2).unwrap(), ["hi"]);
    }
}
