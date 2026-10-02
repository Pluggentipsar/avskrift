"""Check partition tensor parity across the local-attention length boundary."""
import argparse
import importlib.util
import json
from pathlib import Path
import time
import numpy as np
import onnxruntime as ort

spec=importlib.util.spec_from_file_location("probe",Path(__file__).with_name("pianissimo-probe.py"))
probe=importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("--model",type=Path,default=Path(".build-tools/pianissimo/model"))
    p.add_argument("--parts",type=Path,required=True)
    p.add_argument("--audio",type=Path,required=True)
    p.add_argument("--output",type=Path,required=True)
    p.add_argument("--reference-cache",type=Path,help="Save verified original outputs to avoid repeated expensive loads")
    a=p.parse_args()
    source=a.model/'encoder-model.int8.onnx'
    if probe.file_hash(source) != '8dff41d361e89547dbd8e95127f8038841b7489d0a59901d5a62c1033482b7b2':
        raise ValueError('Unexpected original encoder')
    options=probe.session_options(8,'all',True)
    options.enable_cpu_mem_arena=False
    options.add_session_config_entry('session.intra_op.allow_spinning','0')
    options.add_session_config_entry('session.inter_op.allow_spinning','0')
    opts={'sess_options':options,'providers':['CPUExecutionProvider']}
    identity={'schema':1,'ort':ort.__version__,'source_sha256':probe.file_hash(source),
              'audio_sha256':probe.file_hash(a.audio),'cpu_arena':False,'spinning':False}
    cached=False
    if a.reference_cache and (a.reference_cache/'identity.json').exists():
        cached=json.loads((a.reference_cache/'identity.json').read_text(encoding='utf-8'))==identity
        cached=cached and all((a.reference_cache/f'{name}.npz').exists() for name in ('short','boundary','long'))
    start=time.perf_counter()
    original=None if cached else ort.InferenceSession(str(source),**opts)
    original_load=time.perf_counter()-start
    print(f'Original loaded: {original_load:.3f}s',flush=True)
    start=time.perf_counter()
    split=probe.PartitionedEncoder(a.parts,opts)
    split_load=time.perf_counter()-start
    audio=probe.read_audio(a.audio)
    preprocessor=probe.NemoPreprocessorNumpy('nemo128')
    report={'ort':ort.__version__,'original_load_seconds':original_load,'split_load_seconds':split_load,
            'source_audio_sha256':probe.file_hash(a.audio),'manifest_sha256':probe.file_hash(a.parts),
            'cpu_arena':False,'spinning':False,'cached_reference':cached,'rtol':0.001,'atol':0.001,'cases':[]}
    for label,samples in [('short',audio[:16000]),('boundary',np.resize(audio,16000*41+123)),
                          ('long',np.tile(audio,2))]:
        features,lengths=preprocessor(samples[None,:],np.array([len(samples)],dtype=np.int64))
        feeds={'audio_signal':features,'length':lengths}
        outputs=[]
        timings=[]
        for session in (original,split):
            start=time.perf_counter()
            if session is None:
                with np.load(a.reference_cache/f'{label}.npz') as saved:
                    if not np.array_equal(features,saved['features']) or not np.array_equal(lengths,saved['length']):
                        raise ValueError('Cached input features mismatch')
                    outputs.append([saved['outputs'],saved['encoded_lengths']])
            else:
                outputs.append(session.run(['outputs','encoded_lengths'],feeds))
            timings.append(time.perf_counter()-start)
        if a.reference_cache and not cached:
            a.reference_cache.mkdir(parents=True,exist_ok=True)
            np.savez_compressed(a.reference_cache/f'{label}.npz',**feeds,
                                features=features,outputs=outputs[0][0],encoded_lengths=outputs[0][1])
        x,y=outputs[0][0],outputs[1][0]
        equal_lengths=np.array_equal(outputs[0][1],outputs[1][1])
        same_shape=x.shape==y.shape
        finite=np.isfinite(x).all() and np.isfinite(y).all()
        close=same_shape and finite and np.allclose(x,y,rtol=0.001,atol=0.001)
        error=np.abs(x-y) if same_shape else None
        case={'case':label,'audio_seconds':len(samples)/16000,'shape':list(x.shape),'lengths_equal':equal_lengths,
              'finite':bool(finite),'within_tolerance':bool(close),'original_seconds':timings[0],'split_seconds':timings[1],
              'max_abs_error':float(error.max()) if error is not None else None,
              'relative_l2_error':float(np.linalg.norm(x-y)/np.linalg.norm(x)) if same_shape else None}
        report['cases'].append(case)
        a.output.write_text(json.dumps(report,indent=2),encoding='utf-8')
        print(json.dumps(case),flush=True)
    if a.reference_cache and not cached:
        (a.reference_cache/'identity.json').write_text(json.dumps(identity,indent=2),encoding='utf-8')
    if not all(x['within_tolerance'] and x['lengths_equal'] for x in report['cases']):
        raise SystemExit('Encoder tensor parity failed')

if __name__=='__main__': main()
