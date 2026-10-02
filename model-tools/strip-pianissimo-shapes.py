"""Remove optional internal tensor annotations, retaining every operator/weight and public I/O."""
import argparse
import hashlib
import json
from pathlib import Path
import onnx

def digest(path):
    with Path(path).open("rb") as f:
        return hashlib.file_digest(f,"sha256").hexdigest()

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--input",type=Path,default=Path(".build-tools/pianissimo/model/encoder-model.int8.onnx"))
    p.add_argument("--output",type=Path,required=True)
    a=p.parse_args()
    source=digest(a.input)
    if source != "8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2" or a.output.exists():
        p.error("Requires pinned source and a new output path")
    model=onnx.load(a.input)
    count=len(model.graph.value_info)
    model.graph.ClearField("value_info")
    onnx.checker.check_model(model)
    a.output.parent.mkdir(parents=True,exist_ok=True)
    onnx.save(model,a.output)
    report={"source_sha256":source,"candidate_sha256":digest(a.output),"removed_value_info":count,
            "transformation":"clear graph.value_info only","onnx":onnx.__version__}
    a.output.with_suffix(".json").write_text(json.dumps(report,indent=2),encoding="utf-8")
    print(json.dumps(report,indent=2))

if __name__ == "__main__":
    main()
