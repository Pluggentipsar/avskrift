"""Compare native features and transcripts with the independently run NumPy probe."""
import argparse
import importlib.util
import json
from pathlib import Path
import subprocess
import numpy as np
from onnx_asr.preprocessors.numpy_preprocessor import NemoPreprocessorNumpy

spec = importlib.util.spec_from_file_location("probe", Path(__file__).with_name("pianissimo-probe.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", type=Path, required=True)
    parser.add_argument("--python-report", type=Path, required=True)
    parser.add_argument("--rust-report", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timestamp-tolerance", type=float, default=0.0,
                        help="Allowed absolute token-start difference in seconds; default requires exact match")
    args = parser.parse_args()
    reference = json.loads(args.python_report.read_text(encoding="utf-8"))
    native = None if args.rust_report is None else json.loads(args.rust_report.read_text(encoding="utf-8"))
    report = {"feature_absolute_tolerance": 0.001, "timestamp_tolerance": args.timestamp_tolerance, "cases": []}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    for case in reference["cases"]:
        path = Path(case["audio"])
        binary = args.output.parent / (path.stem + "-rust-features.f32")
        subprocess.run([str(args.exe), "features", str(path), str(binary)], check=True)
        audio = probe.read_audio(path)
        expected, _ = NemoPreprocessorNumpy("nemo128")(audio[None, :], np.array([len(audio)], dtype=np.int64))
        actual = np.fromfile(binary, dtype="<f4").reshape(expected.shape)
        error = np.abs(actual - expected)
        check = {"audio": str(path), "max_abs_feature_error": float(error.max()),
                 "mean_abs_feature_error": float(error.mean()), "features_match": bool(np.all(error <= 0.001))}
        if native is not None:
            candidates = [x for x in native["cases"] if x["audio_sha256"] == case["audio_sha256"]]
            if len(candidates) != 1:
                raise ValueError(f"Missing or ambiguous matching WAV: {path}")
            rust = candidates[0]
            # The benchmark reports store the same onnx-asr result schema.
            py_result = case["result"]
            rt, pt = rust["result"]["timestamps"], py_result["timestamps"]
            delta = max((abs(a-b) for a, b in zip(rt, pt)), default=0.0) if len(rt) == len(pt) else None
            check.update({"text_equal": rust["result"]["text"] == py_result["text"],
                          "tokens_equal": rust["result"]["tokens"] == py_result["tokens"],
                          "timestamps_equal": rust["result"]["timestamps"] == py_result["timestamps"],
                          "max_timestamp_delta_seconds": delta,
                          "timestamps_within_tolerance": delta is not None and delta <= args.timestamp_tolerance + 1e-12,
                          "word_errors": probe.word_errors(case["reference"], rust["result"]["text"])})
        report["cases"].append(check)
        print(json.dumps(check, ensure_ascii=True), flush=True)
    args.output.write_text(json.dumps(report, indent=2, ensure_ascii=False), encoding="utf-8")
    if not all(x["features_match"] and all(x.get(k, True) for k in ("text_equal", "tokens_equal", "timestamps_within_tolerance")) for x in report["cases"]):
        raise SystemExit("Native/reference parity check failed; inspect report")


if __name__ == "__main__":
    main()
