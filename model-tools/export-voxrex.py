"""Export KBLab/wav2vec2-large-voxrex-swedish to ONNX for Avskrift's word alignment.

Writes OUT_DIR/{model.onnx, model.fp16.onnx, vocab.json, manifest.json}
and checks each export against PyTorch on a fixed synthetic signal.

Usage: python export-voxrex.py MODEL_DIR OUT_DIR
"""
import hashlib
import json
import shutil
import sys
from pathlib import Path

import numpy as np
import onnx
import onnxruntime as ort
import torch
from onnxruntime.transformers.float16 import convert_float_to_float16
from transformers import Wav2Vec2ForCTC

REVISION = "ca70e31c06a2617bf7fe3b4fb5d387d2b19b2983"


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def main():
    src, out = Path(sys.argv[1]), Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    model = Wav2Vec2ForCTC.from_pretrained(src).eval()
    # Fixed, deterministic test input: 6 s of tones + noise, already normalized like the processor.
    rng = np.random.default_rng(7)
    t = np.arange(6 * 16000) / 16000
    x = (np.sin(2 * np.pi * 220 * t) * np.sin(2 * np.pi * 3 * t) + 0.1 * rng.standard_normal(t.size)).astype(np.float32)
    x = (x - x.mean()) / (x.std() + 1e-7)
    with torch.inference_mode():
        ref = model(torch.from_numpy(x)[None]).logits[0].numpy()

    fp32 = out / "model.onnx"
    torch.onnx.export(model, (torch.from_numpy(x)[None],), str(fp32), input_names=["input_values"],
                      output_names=["logits"], dynamic_axes={"input_values": {0: "batch", 1: "samples"},
                                                             "logits": {0: "batch", 1: "frames"}},
                      opset_version=17, dynamo=False)
    # fp16 is what Avskrift ships: word times match fp32 on real speech; dynamic int8 did not
    # (43 of 425 words > 40 ms off, see docs/TEXTKLIPP-FAS1.md).
    fp16 = out / "model.fp16.onnx"
    onnx.save(convert_float_to_float16(onnx.load(str(fp32)), keep_io_types=True), str(fp16))

    report = {"source": "KBLab/wav2vec2-large-voxrex-swedish", "revision": REVISION, "license": "CC0-1.0",
              "frame_seconds": model.config.inputs_to_logits_ratio / 16000, "blank_id": model.config.pad_token_id,
              "files": {}}
    for path in (fp32, fp16):
        sess = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
        got = sess.run(None, {"input_values": x[None]})[0][0]
        agree = float((got.argmax(-1) == ref.argmax(-1)).mean())
        report["files"][path.name] = {"sha256": sha256(path), "bytes": path.stat().st_size,
                                      "max_abs_logit_diff": float(np.abs(got - ref).max()), "argmax_agreement": agree}
        print(f"{path.name}: {path.stat().st_size / 1e6:.0f} MB, max diff {np.abs(got - ref).max():.4f}, "
              f"argmax agreement {agree:.4f}")
    shutil.copy(src / "vocab.json", out / "vocab.json")
    report["files"]["vocab.json"] = {"sha256": sha256(out / "vocab.json"), "bytes": (out / "vocab.json").stat().st_size}
    json.dump(report, open(out / "manifest.json", "w"), indent=1)


if __name__ == "__main__":
    main()
