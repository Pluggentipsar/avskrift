"""Engångssteg vid bygget: ladda ner KB-BERT NER, exportera till ONNX, kvantisera till int8.

Slutanvändaren behöver ALDRIG köra detta eller ha Python installerat. Resultatet
(model.onnx + tokenizer.json + labels.json) bäddas in i Tauri-appen som resurs.

Körs i en venv – se model-tools/build-model.ps1.
"""
from pathlib import Path
import json
import shutil

from optimum.onnxruntime import ORTModelForTokenClassification
from onnxruntime.quantization import quantize_dynamic, QuantType
from transformers import AutoTokenizer, AutoConfig

MODEL_ID = "KBLab/bert-base-swedish-cased-ner"
ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "resources" / "model"
TMP = Path(__file__).resolve().parent / "_export"


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    TMP.mkdir(parents=True, exist_ok=True)

    print(f"Exporterar {MODEL_ID} till ONNX (fp32)...")
    model = ORTModelForTokenClassification.from_pretrained(MODEL_ID, export=True)
    model.save_pretrained(TMP)

    print("Sparar snabb tokenizer (tokenizer.json)...")
    tok = AutoTokenizer.from_pretrained(MODEL_ID)
    tok.save_pretrained(TMP)

    print("Kvantiserar till int8...")
    quantize_dynamic(str(TMP / "model.onnx"), str(OUT / "model.onnx"), weight_type=QuantType.QInt8)

    shutil.copy(TMP / "tokenizer.json", OUT / "tokenizer.json")

    cfg = AutoConfig.from_pretrained(MODEL_ID)
    labels = {int(k): v for k, v in cfg.id2label.items()}
    (OUT / "labels.json").write_text(json.dumps(labels, ensure_ascii=False, indent=2), encoding="utf-8")

    print(f"\nKlart. Filer i {OUT}:")
    for f in sorted(OUT.iterdir()):
        print(f"  - {f.name}: {f.stat().st_size / 1e6:.1f} MB")
    print("\nEtiketter:", labels)


if __name__ == "__main__":
    main()
