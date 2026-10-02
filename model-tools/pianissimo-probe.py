"""Offline CPU feasibility probe, separate from the shipped Avskrift runtime.

Install requirements-pianissimo.txt in a venv; fetch with fetch-pianissimo.ps1.
Input: mono 16 kHz PCM16 WAV, optional UTF-8 reference in a sibling .txt file.
Outputs include transcripts: keep reports in the ignored .build-tools directory.
"""
import argparse
from dataclasses import asdict
import hashlib
from importlib.metadata import version
import json
from pathlib import Path
import platform
import re
import statistics
import time
import wave

import numpy as np
import onnxruntime as ort
import psutil
from onnx_asr.models.nemo import NemoConformerTdt
from onnx_asr.preprocessors.numpy_preprocessor import NemoPreprocessorNumpy
from onnx_asr.asr import _AsrWithDecoding


class PartitionedEncoder:
    def __init__(self, manifest_path, options):
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        if manifest.get("schema") != 1 or manifest.get("source_sha256") != "8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2":
            raise ValueError("Unrecognized encoder partition provenance")
        self.parts = []
        self.load_seconds = []
        if manifest.get("optimization") == "disabled":
            original_options=options["sess_options"]
            local=ort.SessionOptions()
            local.intra_op_num_threads=original_options.intra_op_num_threads
            local.inter_op_num_threads=original_options.inter_op_num_threads
            local.enable_cpu_mem_arena=original_options.enable_cpu_mem_arena
            local.graph_optimization_level=ort.GraphOptimizationLevel.ORT_DISABLE_ALL
            local.add_session_config_entry("session.x64quantprecision","1")
            local.add_session_config_entry("session.intra_op.allow_spinning","0")
            local.add_session_config_entry("session.inter_op.allow_spinning","0")
            options={**options,"sess_options":local}
        for part in manifest["parts"]:
            if Path(part["file"]).name != part["file"]:
                raise ValueError("Partition file must be a basename")
            path = manifest_path.parent / part["file"]
            if file_hash(path) != part["sha256"]:
                raise ValueError("Partition checksum mismatch")
            start = time.perf_counter()
            session = ort.InferenceSession(str(path), **options)
            self.load_seconds.append(time.perf_counter()-start)
            if [x.name for x in session.get_inputs()] != part["inputs"] or [x.name for x in session.get_outputs()] != part["outputs"]:
                raise ValueError("Partition bindings mismatch")
            self.parts.append((session, part))
            print(f"Loaded {part['file']} in {self.load_seconds[-1]:.3f}s", flush=True)
        if not self.parts:
            raise ValueError("Empty encoder partition")

    def run(self, outputs, feeds):
        values = dict(feeds)
        for i, (session, part) in enumerate(self.parts):
            result = session.run(part["outputs"], {k:values[k] for k in part["inputs"]})
            values.update(zip(part["outputs"], result))
            needed = set(outputs) | {x for _,p in self.parts[i+1:] for x in p["inputs"]}
            values = {k:v for k,v in values.items() if k in needed}
        return [values[k] for k in outputs]


class PianissimoTdt(NemoConformerTdt):
    def __init__(self, files, preprocessor, options, partitions=None):
        if partitions is None:
            super().__init__(files, preprocessor, options)
        else:
            _AsrWithDecoding.__init__(self, files, preprocessor, options)
            self._encoder = PartitionedEncoder(partitions, options)
            self._decoder_joint = ort.InferenceSession(str(files["decoder_joint"]), **options)
    @property
    def _features_size(self):
        # Export config uses `features`, whereas onnx-asr expects `features_size`.
        return 128

    def _encode(self, features, lengths):
        if not np.isfinite(features).all():
            raise ValueError("Non-finite input features")
        encoded, encoded_lengths = super()._encode(features, lengths)
        if not np.isfinite(encoded).all() or np.any(encoded_lengths <= 0):
            raise ValueError(f"Invalid encoder output: lengths={encoded_lengths}, finite={np.isfinite(encoded).all()}")
        self.encoder_diagnostics = {"shape": list(encoded.shape), "lengths": encoded_lengths.tolist(),
                                    "min": float(encoded.min()), "max": float(encoded.max()),
                                    "mean": float(encoded.mean()), "std": float(encoded.std())}
        return encoded, encoded_lengths

    def _decode(self, tokens, state, encoded):
        logits, step, next_state = super()._decode(tokens, state, encoded)
        if not np.isfinite(logits).all():
            raise ValueError("Non-finite decoder logits")
        if not tokens and not hasattr(self, "first_decoder_diagnostics"):
            top = np.argsort(logits)[-5:][::-1]
            self.first_decoder_diagnostics = {"top_ids": top.tolist(), "logits": logits[top].tolist(),
                                              "duration_step": step}
        return logits, step, next_state


def file_hash(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def word_errors(reference, hypothesis):
    """Levenshtein WER: lowercase, punctuation to spaces; preserve number spelling."""
    words = lambda text: re.sub(r"[^\w\s]|_", " ", text.lower()).split()
    ref, hyp = words(reference), words(hypothesis)
    if not ref:
        return {"reference_words": 0, "insertions_on_empty_reference": len(hyp), "wer": None}
    row = list(range(len(hyp) + 1))
    for i, a in enumerate(ref, 1):
        nxt = [i]
        for j, b in enumerate(hyp, 1):
            nxt.append(min(nxt[-1] + 1, row[j] + 1, row[j - 1] + (a != b)))
        row = nxt
    return {"reference_words": len(ref), "word_errors": row[-1], "wer": row[-1] / len(ref)}


def read_audio(path):
    with wave.open(str(path), "rb") as wav:
        if (wav.getnchannels(), wav.getframerate(), wav.getsampwidth()) != (1, 16000, 2):
            raise ValueError(f"{path}: requires mono 16 kHz PCM16 WAV")
        audio = np.frombuffer(wav.readframes(wav.getnframes()), dtype="<i2").astype(np.float32) / 32768
    if len(audio) < 512:
        raise ValueError("Audio is too short for the feature extractor")
    return audio


def session_options(threads, optimization, avx2_precision):
    options = ort.SessionOptions()
    options.intra_op_num_threads = threads
    options.inter_op_num_threads = 1
    if avx2_precision:
        options.add_session_config_entry("session.x64quantprecision", "1")
    options.graph_optimization_level = {"all": ort.GraphOptimizationLevel.ORT_ENABLE_ALL,
                                        "basic": ort.GraphOptimizationLevel.ORT_ENABLE_BASIC,
                                        "disabled": ort.GraphOptimizationLevel.ORT_DISABLE_ALL}[optimization]
    return options


def cached_files(files, hashes, args):
    """Serialize ORT's optimized graphs locally; never distribute this hardware-specific cache."""
    cache = args.cache_dir
    if cache is None:
        return files, 0.0
    cache.mkdir(parents=True, exist_ok=True)
    identity = {"source_hashes": hashes, "ort": ort.__version__, "platform": platform.platform(),
                "processor": platform.processor(), "threads": args.threads,
                "optimization": args.optimization, "avx2_precision": args.avx2_precision}
    manifest = cache / "manifest.json"
    targets = {key: cache / f"{key}.onnx" for key in ("encoder", "decoder_joint")}
    if manifest.exists():
        saved = json.loads(manifest.read_text(encoding="utf-8"))
        if saved.get("identity") == identity and all(
            path.is_file() and file_hash(path) == saved.get("hashes", {}).get(key)
            for key, path in targets.items()
        ):
            return files | targets, 0.0
    start = time.perf_counter()
    for key, target in targets.items():
        options = session_options(args.threads, args.optimization, args.avx2_precision)
        temporary = target.with_suffix(".pending.onnx")
        options.optimized_model_filepath = str(temporary)
        print(f"Preparing local optimized {key} cache...", flush=True)
        session = ort.InferenceSession(str(files[key]), sess_options=options, providers=["CPUExecutionProvider"])
        del session
        temporary.replace(target)
    contents = {"identity": identity, "hashes": {key: file_hash(path) for key, path in targets.items()}}
    pending = manifest.with_suffix(".pending.json")
    pending.write_text(json.dumps(contents, indent=2), encoding="utf-8")
    pending.replace(manifest)
    return files | targets, time.perf_counter() - start


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, default=Path(".build-tools/pianissimo/model"))
    parser.add_argument("--audio", type=Path, nargs="+")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--threads", type=int, default=8)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--optimization", choices=["all", "basic", "disabled"], default="all")
    parser.add_argument("--avx2-precision", action="store_true",
                        help="Use ORT U8U8 precision mode to avoid U8S8 saturation on CPUs without VNNI")
    parser.add_argument("--cache-dir", type=Path, help="Optional local hardware-specific ORT graph cache")
    parser.add_argument("--encoder-candidate", type=Path, help="Explicit experimental encoder, original remains verified")
    parser.add_argument("--candidate-sha256", help="Required checksum when using an experimental encoder")
    parser.add_argument("--encoder-parts", type=Path, help="Verified partition manifest, experimental")
    parser.add_argument("--no-spin", action="store_true")
    parser.add_argument("--prepare-cache-only", action="store_true", help="Prepare/reuse the local cache, without loading it again for inference")
    args = parser.parse_args()
    if args.threads < 1 or args.runs < 2:
        parser.error("threads must be positive and runs >= 2 (first + warm)")
    if bool(args.encoder_candidate) != bool(args.candidate_sha256):
        parser.error("encoder-candidate and candidate-sha256 must be provided together")
    if args.encoder_parts and (args.encoder_candidate or args.cache_dir):
        parser.error("encoder-parts cannot be combined with candidate or graph cache")
    if args.prepare_cache_only and (not args.cache_dir or args.encoder_parts or args.encoder_candidate):
        parser.error("prepare-cache-only requires cache-dir and the original model")
    if not args.prepare_cache_only and not args.audio:
        parser.error("audio is required for inference")

    files = {"encoder": args.model / "encoder-model.int8.onnx",
             "decoder_joint": args.model / "decoder_joint-model.int8.onnx",
             "vocab": args.model / "vocab.txt", "config": args.model / "config.json"}
    expected = {"encoder": "8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2",
                "decoder_joint": "2fb4ef1c1e28839aef70e74a3a2737afdc7460afa1e640ce1c4f9bf9ceadcb51"}
    hashes = {name: file_hash(path) for name, path in files.items()}
    for name, digest in expected.items():
        if hashes[name] != digest:
            raise ValueError(f"Unexpected model hash: {name}")
    config = json.loads(files["config"].read_text(encoding="utf-8-sig"))
    if config.get("features") != 128 or config.get("durations") != [0, 1, 2, 3, 4]:
        raise ValueError("Unsupported preprocessing or TDT duration configuration")
    original_encoder_hash = hashes["encoder"]
    if args.encoder_candidate:
        actual = file_hash(args.encoder_candidate)
        if actual != args.candidate_sha256:
            raise ValueError("Experimental encoder checksum mismatch")
        files["encoder"] = args.encoder_candidate
        hashes["encoder"] = actual
    runtime_files, cache_seconds = cached_files(files, hashes, args)
    if args.prepare_cache_only:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps({"cache_preparation_seconds":cache_seconds,
            "cache_dir":str(args.cache_dir),"source_hashes":hashes,
            "files":{k:str(v) for k,v in runtime_files.items()}},indent=2),encoding="utf-8")
        print(f"Local cache ready; preparation {cache_seconds:.3f}s. No inference run.",flush=True)
        return
    options = session_options(args.threads, args.optimization, args.avx2_precision)
    if args.no_spin or args.encoder_parts:
        options.add_session_config_entry("session.intra_op.allow_spinning", "0")
        options.add_session_config_entry("session.inter_op.allow_spinning", "0")
    start = time.perf_counter()
    model = PianissimoTdt(runtime_files, NemoPreprocessorNumpy,
                         {"sess_options": options, "providers": ["CPUExecutionProvider"]}, args.encoder_parts)
    load_seconds = time.perf_counter() - start
    report = {"engine": "Pianissimo community INT8 / ONNX Runtime CPU",
              "source": json.loads((args.model / "source.json").read_text(encoding="utf-8-sig")),
              "model_sha256": hashes, "threads": args.threads, "load_seconds": load_seconds,
              "original_encoder_sha256": original_encoder_hash,
              "encoder_candidate": str(args.encoder_candidate) if args.encoder_candidate else None,
              "encoder_parts": str(args.encoder_parts) if args.encoder_parts else None,
              "encoder_parts_sha256": file_hash(args.encoder_parts) if args.encoder_parts else None,
              "encoder_part_load_seconds": getattr(model._encoder, "load_seconds", None),
              "spinning": not (args.no_spin or args.encoder_parts),
              "optimization": args.optimization,
              "avx2_precision": args.avx2_precision,
              "cache_dir": str(args.cache_dir) if args.cache_dir else None,
              "cache_preparation_seconds": cache_seconds,
              "platform": platform.platform(), "processor": platform.processor(),
              "ram_bytes": psutil.virtual_memory().total,
              "versions": {p: version(p) for p in ["onnx-asr", "onnxruntime", "numpy"]},
              "timing_scope": "preprocessing + encoder + greedy TDT + token timestamps; excludes WAV loading",
              "cases": []}
    print(f"Model loaded in {load_seconds:.2f}s", flush=True)
    process = psutil.Process()
    for path in args.audio:
        audio = read_audio(path)
        duration = len(audio) / 16000
        if duration > 120:
            raise ValueError("Probe limited to 120 seconds per input; long-form chunking is not implemented")
        results, elapsed = [], []
        for run in range(args.runs):
            if hasattr(model, "first_decoder_diagnostics"):
                del model.first_decoder_diagnostics
            start = time.perf_counter()
            result = next(model.recognize_batch(audio[None, :], np.array([len(audio)], dtype=np.int64)))
            elapsed.append(time.perf_counter() - start)
            results.append(asdict(result))
            print(f"{path.name}: run {run + 1}, {elapsed[-1]:.3f}s / {duration:.2f}s audio", flush=True)
        timestamps = result.timestamps or []
        case = {"audio": str(path.resolve()), "audio_sha256": file_hash(path), "audio_seconds": duration,
                "elapsed_seconds": elapsed, "warm_median_seconds": statistics.median(elapsed[1:]),
                "warm_rtf": statistics.median(elapsed[1:]) / duration,
                "repeat_text_equal": all(r["text"] == results[0]["text"] for r in results),
                "token_timestamps_monotonic": all(a <= b for a, b in zip(timestamps, timestamps[1:])) if timestamps else None,
                "token_timestamps_in_range": all(0 <= t <= duration for t in timestamps) if timestamps else None,
                "has_text": bool(result.text.strip()),
                "encoder_diagnostics": model.encoder_diagnostics,
                "first_decoder_diagnostics": getattr(model, "first_decoder_diagnostics", None),
                "result": results[-1], "rss_after_bytes": process.memory_info().rss,
                "process_peak_wset_bytes": getattr(process.memory_info(), "peak_wset", None)}
        reference = path.with_suffix(".txt")
        if reference.exists():
            text = reference.read_text(encoding="utf-8-sig").strip()
            case["reference"] = text
            case["quality"] = word_errors(text, result.text)
        report["cases"].append(case)
        report["empty_output_on_speech_cases"] = [c["audio"] for c in report["cases"]
                                                 if c.get("reference") and not c["has_text"]]
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Report: {args.output}")
    if report["empty_output_on_speech_cases"]:
        raise SystemExit("FAIL: empty transcript on reference speech; timings are not successful ASR results")


if __name__ == "__main__":
    main()
