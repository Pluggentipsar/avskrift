"""Attribute local ONNX startup to file I/O, encoder and decoder initialization."""
import argparse
import gc
import json
from pathlib import Path
import time

import onnxruntime as ort


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", type=Path, default=Path(".build-tools/pianissimo/model"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--threads", type=int, default=8)
    parser.add_argument("--no-prepacking", action="store_true")
    parser.add_argument("--no-spin", action="store_true")
    parser.add_argument("--flush-to-zero", action="store_true")
    parser.add_argument("--verbose", action="store_true", help="ORT stage logs on stderr")
    parser.add_argument("--encoder-candidate", type=Path)
    args = parser.parse_args()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    report = {"ort": ort.__version__, "threads": args.threads,
              "prepacking": not args.no_prepacking, "spinning": not args.no_spin,
              "flush_to_zero": args.flush_to_zero, "stages": []}
    for name in ("encoder-model.int8.onnx", "decoder_joint-model.int8.onnx"):
        path = args.model / name
        if name.startswith("encoder") and args.encoder_candidate:
            path = args.encoder_candidate
        start = time.perf_counter()
        # Stream the file without keeping a second model-sized allocation alive.
        with path.open("rb") as stream:
            while stream.read(4 * 1024 * 1024):
                pass
        read_seconds = time.perf_counter() - start
        options = ort.SessionOptions()
        if args.verbose:
            options.log_severity_level = 0
        options.intra_op_num_threads = args.threads
        options.inter_op_num_threads = 1
        options.add_session_config_entry("session.x64quantprecision", "1")
        if args.flush_to_zero:
            options.add_session_config_entry("session.set_denormal_as_zero", "1")
        if args.no_prepacking:
            options.add_session_config_entry("session.disable_prepacking", "1")
        if args.no_spin:
            options.add_session_config_entry("session.intra_op.allow_spinning", "0")
            options.add_session_config_entry("session.inter_op.allow_spinning", "0")
        options.enable_profiling = True
        options.profile_file_prefix = str(args.output.parent / (args.output.stem + "-" + name))
        print(f"Loading {name}; sequential file read {read_seconds:.3f}s", flush=True)
        wall, cpu = time.perf_counter(), time.process_time()
        session = ort.InferenceSession(str(path), sess_options=options, providers=["CPUExecutionProvider"])
        stage = {"file": str(path), "read_seconds": read_seconds,
                 "session_wall_seconds": time.perf_counter() - wall,
                 "session_cpu_seconds": time.process_time() - cpu}
        stage["inputs"] = [{"name": x.name, "shape": x.shape, "type": x.type} for x in session.get_inputs()]
        stage["outputs"] = [{"name": x.name, "shape": x.shape, "type": x.type} for x in session.get_outputs()]
        trace = Path(session.end_profiling())
        stage["trace"] = str(trace)
        stage["events"] = json.loads(trace.read_text(encoding="utf-8"))
        report["stages"].append(stage)
        args.output.write_text(json.dumps(report, indent=2), encoding="utf-8")
        print(f"{name}: session {stage['session_wall_seconds']:.3f}s, CPU {stage['session_cpu_seconds']:.3f}s", flush=True)
        del session
        gc.collect()


if __name__ == "__main__":
    main()
