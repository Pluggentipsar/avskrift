"""Partition the pinned encoder at Conformer layer boundaries without changing ops/weights."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import time
import onnx
from onnx import helper

SOURCE = "8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2"

def digest(path):
    with Path(path).open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--input", type=Path, default=Path(".build-tools/pianissimo/model/encoder-model.int8.onnx"))
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--layers-per-part", type=int, default=2, choices=[1,2,3,4,6,8,12])
    p.add_argument("--optimized-cache-manifest",type=Path,
                   help="Partition a verified local optimized encoder; disable further graph optimization")
    a = p.parse_args()
    if a.output.exists():
        p.error("Output directory must not exist")
    source_digest=digest(a.input)
    cache=None
    if a.optimized_cache_manifest:
        cache=json.loads(a.optimized_cache_manifest.read_text(encoding='utf-8'))
        if (cache['identity']['source_hashes']['encoder'] != SOURCE or
            cache['hashes']['encoder'] != source_digest or not cache['identity']['avx2_precision']):
            p.error('Optimized cache provenance/checksum mismatch')
    elif source_digest != SOURCE:
        p.error("Requires the pinned original encoder")
    start = time.perf_counter()
    model = onnx.load(a.input)
    graph = model.graph
    info = {x.name:x for x in [*graph.value_info, *graph.input, *graph.output]}
    constants = {x.output[0]:x for x in graph.node if x.op_type == "Constant"}
    weights = {x.name:x for x in graph.initializer}
    # Export puts constants up front, so place them with their consumers instead.
    groups = [[] for _ in range(24 // a.layers_per_part)]
    compute = [node for node in graph.node if node.op_type != "Constant"]
    stages = []
    producers = {name:i for i,n in enumerate(compute) for name in n.output if name}
    for node in compute:
        match = re.search(r"/layers\.(\d+)/", node.name)
        stages.append(int(match[1]) // a.layers_per_part if match else len(groups)-1)
    # Some layer-labelled shape/mask operations are scheduled before earlier
    # layers. Hoist dependencies to their earliest consumer, preserving the DAG.
    for i in range(len(compute)-1, -1, -1):
        for name in compute[i].input:
            if name in producers:
                parent = producers[name]
                if parent >= i:
                    raise ValueError("Source graph is not topologically ordered")
                stages[parent] = min(stages[parent], stages[i])
    for node, stage in zip(compute, stages):
        groups[stage].append(node)
    consumers = {}
    for i, group in enumerate(groups):
        for node in group:
            for name in node.input:
                consumers.setdefault(name, set()).add(i)
    final = {x.name for x in graph.output}
    available = {x.name for x in graph.input}
    manifest = {"schema":1, "source_sha256":SOURCE,"layers_per_part":a.layers_per_part,
                "onnx":onnx.__version__,"parts":[],"input_sha256":source_digest,
                "optimization":"disabled" if cache else "all", "cache_identity":cache['identity'] if cache else None}
    a.output.mkdir(parents=True)
    for i, nodes in enumerate(groups):
        produced = {x for n in nodes for x in n.output if x}
        needed = {x for n in nodes for x in n.input if x}
        external = needed - produced
        inputs = sorted(external - weights.keys() - constants.keys())
        outputs = sorted(x for x in produced if x in final or any(j > i for j in consumers.get(x, ())))
        if not set(inputs) <= available:
            raise ValueError(f"Unresolved partition inputs: {set(inputs)-available}")
        available.update(outputs)
        missing = (set(inputs) | set(outputs)) - info.keys()
        if missing:
            raise ValueError(f"Missing boundary types: {missing}")
        part_nodes = [constants[x] for x in sorted(external & constants.keys())] + nodes
        part_weights = [weights[x] for x in sorted(external & weights.keys())]
        internal_names = {x for n in part_nodes for x in [*n.input,*n.output]} - set(inputs) - set(outputs)
        part = helper.make_model(helper.make_graph(part_nodes, f"pianissimo_encoder_{i}",
            [info[x] for x in inputs], [info[x] for x in outputs], initializer=part_weights,
            value_info=[value for name,value in info.items() if name in internal_names]),
            opset_imports=model.opset_import, producer_name="Avskrift encoder partition prototype",
            ir_version=model.ir_version)
        # Retain internal annotations too: they influence runtime optimization.
        onnx.checker.check_model(part)
        file = a.output / f"encoder-{i:02d}.onnx"
        onnx.save(part, file)
        manifest["parts"].append({"file":file.name,"sha256":digest(file),"nodes":len(part_nodes),
                                  "inputs":inputs,"outputs":outputs})
        print(f"Part {i}: {len(part_nodes)} nodes, {len(inputs)} inputs, {len(outputs)} outputs",flush=True)
    manifest["seconds"] = time.perf_counter()-start
    (a.output / "manifest.json").write_text(json.dumps(manifest,indent=2),encoding="utf-8")

if __name__ == "__main__":
    main()
