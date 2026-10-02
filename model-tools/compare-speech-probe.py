"""Combine local probe reports; all text is decoded explicitly as UTF-8.

Reports contain transcripts. Do not commit reports from private recordings.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import statistics

spec = importlib.util.spec_from_file_location("probe", Path(__file__).with_name("pianissimo-probe.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pianissimo", type=Path, required=True)
    parser.add_argument("--whisper", type=Path, nargs="+", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    piano = read_json(args.pianissimo)
    rows = []
    for case in piano["cases"]:
        rows.append({"case": Path(case["audio"]).stem, "model": "pianissimo-int8",
                     "audio_sha256": case["audio_sha256"], "audio_seconds": case["audio_seconds"],
                     "load_seconds": piano["load_seconds"],
                     "warm_median_seconds": case["warm_median_seconds"],
                     "repeat_text_equal": case["repeat_text_equal"],
                     "reference": case.get("reference"), "text": case["result"]["text"]})
    for path in args.whisper:
        data = read_json(path)
        audio = Path(data["audio"])
        rows.append({"case": audio.stem, "model": Path(data["model"]).stem,
                     "audio_sha256": probe.file_hash(audio), "audio_seconds": data["audio_seconds"],
                     "model_sha256": probe.file_hash(data["model"]),
                     "load_seconds": data["load_seconds"],
                     "warm_median_seconds": statistics.median(run["seconds"] for run in data["runs"][1:]),
                     "repeat_text_equal": all(run["text"] == data["runs"][0]["text"] for run in data["runs"]),
                     "reference": audio.with_suffix(".txt").read_text(encoding="utf-8-sig").strip(),
                     "text": data["runs"][-1]["text"]})
    for row in rows:
        if row["reference"] is not None:
            row["quality"] = probe.word_errors(row["reference"], row["text"])
        row["has_text"] = bool(row["text"].strip())
    # Refuse a misleading table where a case name denotes different recordings or references.
    for case in {row["case"] for row in rows}:
        same_case = [row for row in rows if row["case"] == case]
        if len({row["audio_sha256"] for row in same_case}) != 1:
            raise ValueError(f"Different audio for case {case}")
        if len({row["reference"] for row in same_case}) != 1:
            raise ValueError(f"Different reference text for case {case}")
    report = {"notes": ["CPU, eight threads. First run excluded from warm median.",
                        "Python ONNX probe versus standalone whisper-rs probe, not an end-to-end app benchmark.",
                        "WER preserves number spelling. Empty ASR output is a failure, never a speed win.",
                        "Synthetic speech only; no general quality conclusion."],
              "pianissimo_source_report": str(args.pianissimo), "results": rows}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    for row in sorted(rows, key=lambda r: (r["case"], r["model"])):
        quality = row.get("quality", {})
        print(f"{row['case']:12} {row['model']:20} {row['warm_median_seconds']:7.3f}s "
              f"errors={quality.get('word_errors')}/{quality.get('reference_words')} text={row['has_text']}")


if __name__ == "__main__":
    main()
