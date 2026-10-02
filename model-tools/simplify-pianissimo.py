"""Conservatively merge identical pure ONNX expressions; retain all model weights.

Writes a separate candidate, never overwrites the source model. Candidate graphs
must pass runtime/transcript validation before use. No shape specialization.
"""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import time

import onnx

# Deliberate allowlist: excludes random/stateful/control-flow/custom-domain ops.
PURE = set("Constant Shape Gather Cast Reshape Mul Concat ConstantOfShape Slice Add Expand Where Equal Range Transpose Squeeze Unsqueeze NonZero ScatterND Sub Pad Div Mod Neg Less Not Identity".split())


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def merge_expressions(model):
    graph = model.graph
    if any(a.type in (onnx.AttributeProto.GRAPH, onnx.AttributeProto.GRAPHS)
           for n in graph.node for a in n.attribute):
        raise ValueError("Subgraphs require scoped alias handling and are not supported")
    outputs = {x.name for x in graph.output}
    aliases, seen, kept = {}, {}, []
    removed = Counter()
    for node in graph.node:
        for i, name in enumerate(node.input):
            node.input[i] = aliases.get(name, name)
        eligible = (node.domain in ("", "ai.onnx") and node.op_type in PURE
                    and len(node.output) == 1 and node.output[0] not in outputs)
        if eligible:
            attributes = []
            for attr in sorted(node.attribute, key=lambda a: a.name):
                canonical = onnx.AttributeProto()
                canonical.CopyFrom(attr)
                if canonical.type == onnx.AttributeProto.TENSOR:
                    # Constant attribute tensor names label storage, not graph values.
                    canonical.t.ClearField("name")
                attributes.append(canonical.SerializeToString(deterministic=True))
            attributes = tuple(attributes)
            key = (node.domain, node.op_type, tuple(node.input), attributes)
            if key in seen:
                aliases[node.output[0]] = seen[key]
                removed[node.op_type] += 1
                continue
            seen[key] = node.output[0]
        kept.append(node)
    del graph.node[:]
    graph.node.extend(kept)
    values = [x for x in graph.value_info if x.name not in aliases]
    del graph.value_info[:]
    graph.value_info.extend(values)
    return dict(removed)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.resolve() == args.input.resolve() or args.output.exists():
        parser.error("Output must be a new file distinct from input")
    start = time.perf_counter()
    model = onnx.load(args.input)
    before = len(model.graph.node)
    weights = [x.SerializeToString(deterministic=True) for x in model.graph.initializer]
    removed = merge_expressions(model)
    assert weights == [x.SerializeToString(deterministic=True) for x in model.graph.initializer]
    del weights
    onnx.checker.check_model(model)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    onnx.save(model, args.output)
    report = {"onnx": onnx.__version__, "source_sha256": digest(args.input),
              "candidate_sha256": digest(args.output), "nodes_before": before,
              "nodes_after": len(model.graph.node), "removed_by_op": removed,
              "initializers_unchanged": True, "seconds": time.perf_counter() - start}
    args.output.with_suffix(".cse.json").write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps(report, indent=2), flush=True)


if __name__ == "__main__":
    main()
