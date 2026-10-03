//! Speaker diarisation: pyannote segmentation + a speaker-embedding model, with agglomerative
//! clustering into speakers.
//!
//! Produces speaker "turns" — time ranges each attributed to a cluster — which `align` then matches
//! against the transcript segments.
//!
//! This is a port of sherpa-onnx 1.12.9's `OfflineSpeakerDiarizationPyannoteImpl::Process`. We used
//! to call that through `sherpa_rs::diarize`, but its final labelling step calls
//! `TopkIndex(count, num_clusters, speakers_in_frame)`, and `TopkIndex` ran `std::partial_sort` with
//! `topk > size` whenever a frame had overlapping speech (2 speakers) but clustering produced one
//! cluster: `num_speakers = 1`, or fewer embeddings than requested speakers (fastcluster's
//! `cutree_k` then puts everything in cluster 0). That reads and writes past a heap buffer and
//! killed the whole app with 0xc0000005. Upstream fixed it in sherpa-onnx #3628 (1.13.x), which no
//! sherpa-rs release ships. Here the segmentation model runs through `ort`, sherpa-onnx only computes
//! speaker embeddings (plain C API), and the labelling below clamps `k` to the number of clusters.
//! Output is otherwise identical to sherpa-onnx's (verified on a 10 min recording, 2 and 3 speakers).

use std::path::Path;

use anyhow::{anyhow, Result};
use ort::{session::Session, value::Tensor};
use sherpa_rs::sherpa_rs_sys as sys;

/// A contiguous time range attributed to one speaker cluster.
#[derive(Debug, Clone)]
pub struct SpeakerTurn {
    pub start: f64,
    pub end: f64,
    /// Cluster index (0-based).
    pub speaker: usize,
}

/// Speaker count when none is given. sherpa-rs turned `num_clusters: None` into 4 clusters (its
/// distance threshold was never used), so that is what "auto" has always meant here.
const AUTO_SPEAKERS: usize = 4;
const MIN_DURATION_ON: f32 = 0.3;
const MIN_DURATION_OFF: f32 = 0.5;
/// A (chunk, local speaker) pair needs this many active frames to get an embedding.
const MIN_FRAMES: usize = 10;

/// Run diarisation over 16 kHz mono `samples`.
///
/// `num_speakers` forces a fixed speaker count when `Some`; `None` uses [`AUTO_SPEAKERS`]. The
/// pyannote segmentation + embedding ONNX models come from [`crate::models`].
pub fn diarize(
    segmentation_model: &Path,
    embedding_model: &Path,
    samples: &[f32],
    num_speakers: Option<usize>,
    progress: &dyn Fn(&str),
) -> Result<Vec<SpeakerTurn>> {
    for (p, what) in [(segmentation_model, "segmenterings"), (embedding_model, "embedding")] {
        if !p.exists() {
            return Err(anyhow!("diariserings-{what}modellen saknas: {}", p.display()));
        }
    }

    crate::work::check()?;
    progress("Förbereder diarisering…");
    let mut segmenter = Segmenter::load(segmentation_model)?;
    let mut embedder = Embedder::load(embedding_model)?;

    progress("Identifierar talare…");
    let labels = segmenter.run(samples)?;
    let meta = segmenter.meta;
    let turns = if labels.len() == 1 {
        // One window: the segmentation model's local speakers are the answer, no clustering.
        segments(&labels[0])
    } else {
        let per_frame = speakers_per_frame(&meta, &labels);
        if per_frame.iter().all(|&k| k == 0) {
            return Err(anyhow!("diariseringen hittade inga talare"));
        }
        let mut pairs = Vec::new();
        let mut embeddings = Vec::new();
        for (pair, ranges) in chunk_speaker_ranges(&meta, &labels) {
            crate::work::check()?;
            if let Some(e) = embedder.compute(samples, &ranges, meta.sample_rate)? {
                pairs.push(pair);
                embeddings.push(e);
            }
        }
        if embeddings.is_empty() {
            return Err(anyhow!("diariseringen hittade inga talare"));
        }
        let clusters = cluster(embeddings, num_speakers.unwrap_or(AUTO_SPEAKERS));
        let count = speaker_count(&meta, &labels, &pairs, &clusters, samples.len());
        segments(&finalize(&count, &per_frame))
    };

    crate::work::check()?;
    let scale = meta.receptive_field_shift as f32 / meta.sample_rate as f32;
    let offset = (0.5 * meta.receptive_field_size as f64 / meta.sample_rate as f64) as f32;
    let mut turns: Vec<SpeakerTurn> = turns
        .into_iter()
        .map(|(start, end, speaker)| {
            let (start, end) = (start as f32 * scale + offset, end as f32 * scale + offset);
            (start, end, speaker)
        })
        .collect::<Vec<_>>()
        .chunk_by(|a, b| a.2 == b.2)
        .flat_map(merge_gaps)
        .filter(|&(s, e, _)| e - s > MIN_DURATION_ON)
        .map(|(s, e, speaker)| SpeakerTurn { start: s as f64, end: e as f64, speaker })
        .collect();
    if turns.is_empty() {
        return Err(anyhow!("diariseringen hittade inga talare"));
    }
    turns.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    Ok(turns)
}

/// Row-major 0/1 matrix, `rows` frames × `cols` speakers.
#[derive(Debug, Clone, PartialEq)]
struct Grid {
    rows: usize,
    cols: usize,
    data: Vec<u8>,
}

impl Grid {
    fn zeros(rows: usize, cols: usize) -> Self {
        Self { rows, cols, data: vec![0; rows * cols] }
    }
    fn at(&self, r: usize, c: usize) -> u8 {
        self.data[r * self.cols + c]
    }
    fn row(&self, r: usize) -> &[u8] {
        &self.data[r * self.cols..(r + 1) * self.cols]
    }
}

/// The segmentation model's custom metadata (as read by sherpa-onnx).
#[derive(Debug, Clone, Copy)]
struct Meta {
    sample_rate: i32,
    window_size: usize,
    window_shift: usize,
    receptive_field_size: i32,
    receptive_field_shift: usize,
    num_speakers: usize,
}

struct Segmenter {
    session: Session,
    meta: Meta,
    /// Powerset class -> per-speaker activity (`num_classes` × `num_speakers`).
    powerset: Grid,
}

impl Segmenter {
    fn load(path: &Path) -> Result<Self> {
        let session = Session::builder()?
            .with_independent_thread_pool()
            .map_err(|e| anyhow!("{e}"))?
            .with_intra_threads(1)
            .map_err(|e| anyhow!("{e}"))?
            .commit_from_file(path)
            .map_err(|e| anyhow!("kunde inte initiera diariseringen: {e}"))?;
        let md = session.metadata()?;
        let get = |key: &str| -> Result<i32> {
            md.custom(key)
                .and_then(|v| v.trim().parse().ok())
                .ok_or_else(|| anyhow!("segmenteringsmodellen saknar metadata '{key}'"))
        };
        let window_size = get("window_size")?;
        let meta = Meta {
            sample_rate: get("sample_rate")?,
            window_size: window_size as usize,
            window_shift: (0.1 * window_size as f64) as usize,
            receptive_field_size: get("receptive_field_size")?,
            receptive_field_shift: get("receptive_field_shift")? as usize,
            num_speakers: get("num_speakers")? as usize,
        };
        let powerset = powerset(get("num_classes")? as usize, meta.num_speakers, get("powerset_max_classes")?)?;
        drop(md);
        Ok(Self { session, meta, powerset })
    }

    /// Per-window speaker activity (frames × local speakers) for 10 s windows every 1 s; the last
    /// partial window is zero-padded.
    fn run(&mut self, samples: &[f32]) -> Result<Vec<Grid>> {
        let (n, ws, shift) = (samples.len(), self.meta.window_size, self.meta.window_shift);
        if n == 0 {
            return Err(anyhow!("diariseringen fick inget ljud"));
        }
        let mut starts: Vec<usize> =
            if n <= ws { vec![0] } else { (0..=(n - ws) / shift).map(|i| i * shift).collect() };
        if n > ws && !(n - ws).is_multiple_of(shift) {
            starts.push(starts.len() * shift);
        }
        let mut out = Vec::with_capacity(starts.len());
        let mut window = vec![0f32; ws];
        for start in starts {
            crate::work::check()?;
            let src = &samples[start..(start + ws).min(n)];
            window.fill(0.0);
            window[..src.len()].copy_from_slice(src);
            let input = Tensor::from_array(([1usize, 1, ws], window.clone()))?;
            let outputs = self.session.run(ort::inputs![input])?;
            let (shape, scores) = outputs[0].try_extract_tensor::<f32>()?;
            let (frames, classes) = (shape[1] as usize, shape[2] as usize);
            let mut grid = Grid::zeros(frames, self.powerset.cols);
            for f in 0..frames {
                let row = &scores[f * classes..(f + 1) * classes];
                // First maximum wins, like Eigen's maxCoeff.
                let best = (1..classes).fold(0, |b, c| if row[c] > row[b] { c } else { b });
                grid.data[f * grid.cols..(f + 1) * grid.cols].copy_from_slice(self.powerset.row(best));
            }
            out.push(grid);
        }
        Ok(out)
    }
}

/// pyannote powerset mapping: class 0 = silence, then each single speaker, then each pair.
fn powerset(num_classes: usize, num_speakers: usize, max_classes: i32) -> Result<Grid> {
    let mut m = Grid::zeros(num_classes, num_speakers);
    let mut k = 1;
    let mut set = |k: usize, j: usize| -> Result<()> {
        *m.data.get_mut(k * num_speakers + j).ok_or_else(|| anyhow!("ogiltig segmenteringsmodell"))? = 1;
        Ok(())
    };
    for i in 1..=max_classes {
        match i {
            1 => {
                for j in 0..num_speakers {
                    set(k, j)?;
                    k += 1;
                }
            }
            2 => {
                for j in 0..num_speakers {
                    for l in j + 1..num_speakers {
                        set(k, j)?;
                        set(k, l)?;
                        k += 1;
                    }
                }
            }
            _ => return Err(anyhow!("segmenteringsmodellen stöds inte (powerset_max_classes = {max_classes})")),
        }
    }
    Ok(m)
}

/// Global frame count covered by all windows, and the first global frame of window `i`
/// (float arithmetic as in sherpa-onnx so frame alignment matches it exactly).
fn frame_layout(meta: &Meta, num_chunks: usize) -> (usize, impl Fn(usize) -> usize + '_) {
    let total = (meta.window_size + (num_chunks - 1) * meta.window_shift) / meta.receptive_field_shift + 1;
    let start = move |i: usize| {
        ((i as f32 * meta.window_shift as f32 / meta.receptive_field_shift as f32) as f64 + 0.5) as usize
    };
    (total, start)
}

/// Rounded average number of active speakers per global frame.
fn speakers_per_frame(meta: &Meta, labels: &[Grid]) -> Vec<usize> {
    let (total, start) = frame_layout(meta, labels.len());
    let mut count = vec![0f32; total];
    let mut weight = vec![0f32; total];
    for (i, l) in labels.iter().enumerate() {
        for f in 0..l.rows {
            let Some(g) = count.get_mut(start(i) + f) else { break };
            *g += l.row(f).iter().map(|&x| x as f32).sum::<f32>();
            weight[start(i) + f] += 1.0;
        }
    }
    count.iter().zip(&weight).map(|(c, w)| (c / (w + 1e-12) + 0.5) as usize).collect()
}

/// For every (window, local speaker) with enough single-speaker frames: the sample ranges where that
/// speaker talks alone. Overlapped frames are excluded so embeddings stay clean.
/// `(window, local speaker)` and the sample ranges where that speaker talks alone.
type SpeakerRanges = ((usize, usize), Vec<(i64, i64)>);

fn chunk_speaker_ranges(meta: &Meta, labels: &[Grid]) -> Vec<SpeakerRanges> {
    let mut out = Vec::new();
    for (chunk, l) in labels.iter().enumerate() {
        let offset = (chunk * meta.window_shift) as f32;
        let to_sample = |frame: usize| (frame as f32 / l.rows as f32 * meta.window_size as f32 + offset) as i64;
        let alone = |f: usize, s: usize| l.at(f, s) == 1 && l.row(f).iter().map(|&x| x as u32).sum::<u32>() < 2;
        for s in 0..l.cols {
            if (0..l.rows).filter(|&f| alone(f, s)).count() < MIN_FRAMES {
                continue;
            }
            let mut ranges = Vec::new();
            let mut since = None;
            for f in 0..l.rows {
                match (alone(f, s), since) {
                    (true, None) => since = Some(f),
                    (false, Some(b)) => {
                        ranges.push((to_sample(b), to_sample(f)));
                        since = None;
                    }
                    _ => {}
                }
            }
            if let Some(b) = since {
                ranges.push((to_sample(b), to_sample(l.rows - 1)));
            }
            out.push(((chunk, s), ranges));
        }
    }
    out
}

/// sherpa-onnx's speaker-embedding extractor (same model, threads and provider as its diarizer).
struct Embedder {
    ptr: *const sys::SherpaOnnxSpeakerEmbeddingExtractor,
    dim: usize,
}

impl Embedder {
    fn load(model: &Path) -> Result<Self> {
        let model = std::ffi::CString::new(model.to_str().ok_or_else(|| anyhow!("ogiltig sökväg"))?)?;
        let provider = std::ffi::CString::new("cpu")?;
        let config = sys::SherpaOnnxSpeakerEmbeddingExtractorConfig {
            model: model.as_ptr(),
            num_threads: 1,
            debug: 0,
            provider: provider.as_ptr(),
        };
        // SAFETY: the config's strings outlive the call; sherpa copies what it keeps.
        let ptr = unsafe { sys::SherpaOnnxCreateSpeakerEmbeddingExtractor(&config) };
        if ptr.is_null() {
            return Err(anyhow!("kunde inte initiera diariseringen: embeddingmodellen kunde inte laddas"));
        }
        let dim = unsafe { sys::SherpaOnnxSpeakerEmbeddingExtractorDim(ptr) }.max(0) as usize;
        Ok(Self { ptr, dim })
    }

    /// Embedding of the concatenated `ranges`; `None` when too short or the model returns NaN.
    fn compute(&mut self, samples: &[f32], ranges: &[(i64, i64)], sample_rate: i32) -> Result<Option<Vec<f32>>> {
        let n = samples.len() as i64;
        // SAFETY: every pointer passed is valid for the call; stream and embedding are freed here.
        unsafe {
            let stream = sys::SherpaOnnxSpeakerEmbeddingExtractorCreateStream(self.ptr);
            if stream.is_null() {
                return Err(anyhow!("diariseringen misslyckades: kunde inte skapa embeddingström"));
            }
            for &(a, b) in ranges {
                let (a, b) = (a.clamp(0, n), b.min(n));
                if b > a {
                    let part = &samples[a as usize..b as usize];
                    sys::SherpaOnnxOnlineStreamAcceptWaveform(stream, sample_rate, part.as_ptr(), part.len() as i32);
                }
            }
            sys::SherpaOnnxOnlineStreamInputFinished(stream);
            let mut out = None;
            if sys::SherpaOnnxSpeakerEmbeddingExtractorIsReady(self.ptr, stream) != 0 {
                let e = sys::SherpaOnnxSpeakerEmbeddingExtractorComputeEmbedding(self.ptr, stream);
                if !e.is_null() {
                    let v = std::slice::from_raw_parts(e, self.dim).to_vec();
                    sys::SherpaOnnxSpeakerEmbeddingExtractorDestroyEmbedding(e);
                    if v.iter().all(|x| !x.is_nan()) {
                        out = Some(v);
                    }
                }
            }
            sys::SherpaOnnxDestroyOnlineStream(stream);
            Ok(out)
        }
    }
}

impl Drop for Embedder {
    fn drop(&mut self) {
        // SAFETY: created in `load`, destroyed once.
        unsafe { sys::SherpaOnnxDestroySpeakerEmbeddingExtractor(self.ptr) }
    }
}

/// Complete-linkage agglomerative clustering on cosine distance into `k` clusters (fewer only when
/// there are fewer embeddings). Labels are numbered by first appearance.
fn cluster(mut embeddings: Vec<Vec<f32>>, k: usize) -> Vec<usize> {
    let n = embeddings.len();
    if n <= 1 {
        return vec![0; n];
    }
    for e in &mut embeddings {
        let norm2: f32 = e.iter().map(|x| x * x).sum();
        if norm2 > 0.0 {
            let norm = norm2.sqrt();
            e.iter_mut().for_each(|x| *x /= norm);
        }
    }
    let mut dist = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in i + 1..n {
            let dot: f32 = embeddings[i].iter().zip(&embeddings[j]).map(|(a, b)| a * b).sum();
            dist.push((1.0 - dot as f64).max(0.0) as f32);
        }
    }
    drop(embeddings);
    let mut merges = complete_linkage(n, dist);
    merges.sort_by(|a, b| a.2.total_cmp(&b.2));
    let steps = n - k.clamp(1, n);
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for &(a, b, _) in &merges[..steps] {
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        parent[ra] = rb;
    }
    let mut label_of = std::collections::HashMap::new();
    (0..n)
        .map(|i| {
            let r = root(&mut parent, i);
            let next = label_of.len();
            *label_of.entry(r).or_insert(next)
        })
        .collect()
}

/// Nearest-neighbour-chain complete linkage (as fastcluster's `NN_chain_core`). `dist` is the
/// condensed upper triangle. Returns `n - 1` merges `(a, b, height)` between observation indices.
fn complete_linkage(n: usize, mut dist: Vec<f32>) -> Vec<(usize, usize, f32)> {
    let idx = |i: usize, j: usize| {
        let (i, j) = if i < j { (i, j) } else { (j, i) };
        i * n - i * (i + 1) / 2 + (j - i - 1)
    };
    let mut active: Vec<usize> = (0..n).collect();
    let mut chain: Vec<usize> = Vec::new();
    let mut merges = Vec::with_capacity(n - 1);
    while active.len() > 1 {
        if chain.is_empty() {
            chain.push(active[0]);
        }
        let (a, b, h) = loop {
            let a = chain[chain.len() - 1];
            let prev = (chain.len() >= 2).then(|| chain[chain.len() - 2]);
            let (mut b, mut best) = prev.map_or((usize::MAX, f32::INFINITY), |p| (p, dist[idx(a, p)]));
            for &x in &active {
                if x != a && dist[idx(a, x)] < best {
                    best = dist[idx(a, x)];
                    b = x;
                }
            }
            if Some(b) == prev {
                chain.truncate(chain.len() - 2);
                break (a, b, best);
            }
            chain.push(b);
        };
        // The merged cluster lives on in slot `hi`; complete linkage keeps the larger distance.
        let (lo, hi) = (a.min(b), a.max(b));
        active.retain(|&x| x != lo);
        for &x in &active {
            if x != hi {
                dist[idx(hi, x)] = dist[idx(hi, x)].max(dist[idx(lo, x)]);
            }
        }
        merges.push((lo, hi, h));
    }
    merges
}

/// Per global frame and cluster: in how many overlapping windows that cluster is active.
fn speaker_count(meta: &Meta, labels: &[Grid], pairs: &[(usize, usize)], clusters: &[usize], n: usize) -> Grid {
    let k = clusters.iter().max().map_or(1, |m| m + 1);
    let (total, start) = frame_layout(meta, labels.len());
    let mut count = vec![0u32; total * k];
    let mut chunk_pairs = pairs.iter().zip(clusters).peekable();
    let mut active = vec![0u8; k];
    for (i, l) in labels.iter().enumerate() {
        let mine: Vec<(usize, usize)> =
            std::iter::from_fn(|| chunk_pairs.next_if(|((c, _), _)| *c == i).map(|((_, s), &m)| (*s, m))).collect();
        for f in 0..l.rows {
            let g = start(i) + f;
            if g >= total {
                break;
            }
            active.fill(0);
            for &(s, m) in &mine {
                active[m] |= l.at(f, s);
            }
            for m in 0..k {
                count[g * k + m] += active[m] as u32;
            }
        }
    }
    // Drop frames that only cover the zero padding of the last window.
    let ws = meta.window_size;
    let rows = if n > ws && !(n - ws).is_multiple_of(meta.window_shift) {
        (n / meta.receptive_field_shift + 1).min(total)
    } else {
        total
    };
    count.truncate(rows * k);
    Grid { rows, cols: k, data: count.into_iter().map(|c| c.min(u8::MAX as u32) as u8).collect() }
}

/// Mark the `per_frame[i]` most active clusters in each frame. `k` is clamped to the cluster count —
/// the sherpa-onnx 1.12.9 crash was exactly this case (2 overlapping speakers, 1 cluster).
fn finalize(count: &Grid, per_frame: &[usize]) -> Grid {
    let mut out = Grid::zeros(count.rows, count.cols);
    let mut order: Vec<usize> = Vec::with_capacity(count.cols);
    for i in 0..count.rows {
        let k = per_frame.get(i).copied().unwrap_or(0).min(count.cols);
        if k == 0 {
            continue;
        }
        let row = count.row(i);
        order.clear();
        order.extend(0..count.cols);
        // Ties keep the lower cluster index.
        order.sort_by(|&a, &b| row[b].cmp(&row[a]));
        for &m in &order[..k] {
            out.data[i * count.cols + m] = 1;
        }
    }
    out
}

/// Active runs per speaker column as `(start_frame, end_frame, speaker)`, speaker-major.
fn segments(labels: &Grid) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    if labels.rows == 0 {
        return out;
    }
    for s in 0..labels.cols {
        let mut since = (labels.at(0, s) > 0).then_some(0);
        for f in 1..labels.rows {
            match (labels.at(f, s) > 0, since) {
                (false, Some(b)) => {
                    out.push((b, f, s));
                    since = None;
                }
                (true, None) => since = Some(f),
                _ => {}
            }
        }
        if let Some(b) = since {
            out.push((b, labels.rows - 1, s));
        }
    }
    out
}

/// Join one speaker's consecutive turns separated by a gap of at most [`MIN_DURATION_OFF`].
fn merge_gaps(turns: &[(f32, f32, usize)]) -> Vec<(f32, f32, usize)> {
    let mut out: Vec<(f32, f32, usize)> = Vec::with_capacity(turns.len());
    for &t in turns {
        match out.last_mut() {
            Some(last) if last.1 < t.0 && last.1 + MIN_DURATION_OFF >= t.0 => last.1 = t.1,
            _ => out.push(t),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // pyannote segmentation-3.0 as exported for sherpa-onnx.
    const META: Meta = Meta {
        sample_rate: 16000,
        window_size: 160000,
        window_shift: 16000,
        receptive_field_size: 991,
        receptive_field_shift: 270,
        num_speakers: 3,
    };

    /// Windows where local speakers 0 and 1 overlap for a stretch, as in real conversation: speaker 0
    /// talks in global frames 0..300 + 59·chunks/2, speaker 1 from 250 + 59·chunks/2 on.
    fn overlapping_windows(chunks: usize) -> Vec<Grid> {
        let (_, start) = frame_layout(&META, chunks);
        let middle = start(chunks / 2);
        (0..chunks)
            .map(|i| {
                let mut g = Grid::zeros(589, META.num_speakers);
                for f in 0..589 {
                    let global = start(i) + f;
                    if global < middle + 300 {
                        g.data[f * 3] = 1;
                    }
                    if global >= middle + 250 {
                        g.data[f * 3 + 1] = 1;
                    }
                }
                g
            })
            .collect()
    }

    /// Regression for the 0xc0000005 crash: frames with 2 active speakers but only 1 cluster.
    #[test]
    fn overlap_with_single_cluster() {
        let labels = overlapping_windows(12);
        let n = META.window_size + 11 * META.window_shift + 5000;
        let per_frame = speakers_per_frame(&META, &labels);
        assert!(per_frame.contains(&2), "the fixture must contain overlapped frames");
        let pairs: Vec<(usize, usize)> = chunk_speaker_ranges(&META, &labels).into_iter().map(|(p, _)| p).collect();
        assert!(pairs.iter().any(|p| p.1 == 1) && pairs.iter().any(|p| p.1 == 0));
        let clusters = vec![0; pairs.len()];
        let count = speaker_count(&META, &labels, &pairs, &clusters, n);
        assert_eq!(count.cols, 1);
        let fin = finalize(&count, &per_frame);
        assert!(fin.data.contains(&1));
        assert!(segments(&fin).iter().all(|&(_, _, s)| s == 0));
    }

    #[test]
    fn overlap_frames_are_excluded_from_embeddings() {
        let labels = overlapping_windows(1);
        let ranges = chunk_speaker_ranges(&META, &labels);
        let r0 = &ranges[0].1;
        let r1 = &ranges[1].1;
        // Speaker 0 alone in frames 0..250, speaker 1 alone in frames 300..589.
        assert_eq!(r0, &vec![(0, (250.0f32 / 589.0 * 160000.0) as i64)]);
        assert_eq!(r1[0].0, (300.0f32 / 589.0 * 160000.0) as i64);
    }

    #[test]
    fn clustering_fixed_count() {
        let a = vec![1.0, 0.0, 0.0];
        let a2 = vec![0.9, 0.1, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let b2 = vec![0.1, 0.9, 0.05];
        let c = vec![0.0, 0.0, 1.0];
        let all = vec![a.clone(), b.clone(), a2.clone(), c.clone(), b2.clone()];
        assert_eq!(cluster(all.clone(), 3), vec![0, 1, 0, 2, 1]);
        let two = cluster(all.clone(), 2);
        assert!(two[0] == two[2] && two[1] == two[4] && two.iter().max() == Some(&1));
        assert_eq!(cluster(all.clone(), 1), vec![0; 5]);
        // More speakers requested than embeddings: every embedding its own speaker.
        assert_eq!(cluster(vec![a.clone(), b.clone()], 4), vec![0, 1]);
        assert_eq!(cluster(vec![a.clone(), a2], 1), vec![0, 0]);
        assert_eq!(cluster(vec![a], 2), vec![0]);
    }

    #[test]
    fn complete_linkage_uses_farthest_member() {
        // Points on a line at 0, 1, 2.1, 10: single linkage would chain 0-1-2.1 at height 1.1,
        // complete linkage merges {0,1} with 2.1 at height 2.1.
        let p = [0.0f32, 1.0, 2.1, 10.0];
        let mut d = Vec::new();
        for i in 0..4 {
            for j in i + 1..4 {
                d.push((p[i] - p[j]).abs());
            }
        }
        let mut m = complete_linkage(4, d);
        m.sort_by(|a, b| a.2.total_cmp(&b.2));
        let h: Vec<f32> = m.iter().map(|x| x.2).collect();
        assert_eq!(h, vec![1.0, 2.1, 10.0]);
    }

    #[test]
    fn segment_runs_and_gap_merging() {
        let mut g = Grid::zeros(10, 2);
        for f in [0, 1, 2, 6, 7, 9] {
            g.data[f * 2] = 1;
        }
        assert_eq!(segments(&g), vec![(0, 3, 0), (6, 8, 0), (9, 9, 0)]);
        let turns = [(0.0, 1.0, 0), (1.4, 2.0, 0), (3.0, 4.0, 0)];
        assert_eq!(merge_gaps(&turns), vec![(0.0, 2.0, 0), (3.0, 4.0, 0)]);
    }

    #[test]
    fn powerset_mapping() {
        let m = powerset(7, 3, 2).unwrap();
        let rows: Vec<&[u8]> = (0..7).map(|r| m.row(r)).collect();
        assert_eq!(rows, vec![&[0, 0, 0][..], &[1, 0, 0], &[0, 1, 0], &[0, 0, 1], &[1, 1, 0], &[1, 0, 1], &[0, 1, 1]]);
    }

    /// Long-audio diarisation through the real models, repeated (the old sherpa-onnx path crashed
    /// intermittently here). `AVSKRIFT_DIARIZE_TEST_AUDIO` = a ~10 min 16 kHz speech file.
    #[test]
    #[ignore]
    fn long_audio_repeated() {
        let wav = std::path::PathBuf::from(std::env::var("AVSKRIFT_DIARIZE_TEST_AUDIO").unwrap());
        let audio = crate::audio::load(&wav).unwrap();
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/diarization");
        let (seg, emb) = (resources.join("segmentation.onnx"), resources.join("embedding.onnx"));
        let runs: usize = std::env::var("AVSKRIFT_DIARIZE_RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(4);
        for (i, speakers) in [Some(1), None, Some(2)].into_iter().cycle().take(runs.max(3)).enumerate() {
            let t = std::time::Instant::now();
            let turns = diarize(&seg, &emb, &audio.samples, speakers, &|_| {}).unwrap();
            let max = turns.iter().map(|t| t.speaker).max().unwrap();
            println!("run {i}: {speakers:?} -> {} turns, {} speakers, {:.1?}", turns.len(), max + 1, t.elapsed());
            assert!(turns.iter().all(|t| t.start < t.end && t.end <= audio.duration_s + 1.0));
            assert!(max < speakers.unwrap_or(AUTO_SPEAKERS));
        }
    }
}
