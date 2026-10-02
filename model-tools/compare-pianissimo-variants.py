"""Compare speech reports with matching input hashes; exact tokens/text required."""
import argparse
import json
from pathlib import Path

def compare(reference, candidate, tolerance=0.0):
    cases = []
    for old in reference["cases"]:
        matched = [x for x in candidate["cases"] if x["audio_sha256"] == old["audio_sha256"]]
        if len(matched) != 1:
            raise ValueError("Missing/ambiguous matching input hash")
        new = matched[0]
        a,b = old["result"],new["result"]
        times_a,times_b = a["timestamps"],b["timestamps"]
        delta = max((abs(x-y) for x,y in zip(times_a,times_b)),default=0.0) if len(times_a)==len(times_b) else None
        cases.append({"audio":Path(old["audio"]).stem,"text_equal":a["text"]==b["text"],
            "tokens_equal":a["tokens"]==b["tokens"],"max_timestamp_delta_seconds":delta,
            "timestamps_match":delta is not None and delta<=tolerance+1e-12,
            "reference_seconds":old["warm_median_seconds"],"candidate_seconds":new["warm_median_seconds"]})
    return {"reference_load_seconds":reference["load_seconds"],"candidate_load_seconds":candidate["load_seconds"],
            "timestamp_tolerance":tolerance,"cases":cases,
            "pass":all(x["text_equal"] and x["tokens_equal"] and x["timestamps_match"] for x in cases)}

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--reference",type=Path,required=True)
    p.add_argument("--candidate",type=Path,nargs="+",required=True)
    p.add_argument("--output",type=Path,required=True)
    p.add_argument("--timestamp-tolerance",type=float,default=0.0)
    a=p.parse_args()
    ref=json.loads(a.reference.read_text(encoding="utf-8"))
    results={str(path):compare(ref,json.loads(path.read_text(encoding="utf-8")),a.timestamp_tolerance) for path in a.candidate}
    a.output.write_text(json.dumps(results,indent=2,ensure_ascii=False),encoding="utf-8")
    print(json.dumps(results,indent=2,ensure_ascii=True))
    if not all(x["pass"] for x in results.values()):
        raise SystemExit("Parity failed; inspect differences")

if __name__ == "__main__":
    main()
