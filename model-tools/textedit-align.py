"""CTC forced alignment of a Whisper transcript with Swedish wav2vec2 (KBLab VoxRex).

Prototype for text-based video editing: gives each Whisper word a precise start/end
from the acoustic model instead of Whisper's token timestamps. Pure numpy Viterbi,
so the same algorithm can later be ported to Rust/ort.

Usage: python textedit-align.py MODEL_DIR AUDIO16K.wav WHISPER.json OUT.json
"""
import json
import sys
import time
import wave

import numpy as np
import torch
from transformers import Wav2Vec2ForCTC

CHUNK_S, PAD_S, RATE = 20.0, 1.0, 16000


def load_wav(path):
    with wave.open(path) as w:
        assert w.getframerate() == RATE and w.getnchannels() == 1 and w.getsampwidth() == 2
        return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768.0


def emissions(model, audio):
    """Log-probs per 20 ms frame, computed in padded chunks to bound attention memory."""
    hop = model.config.inputs_to_logits_ratio  # 320 samples = 20 ms
    n_frames = len(audio) // hop
    out = np.zeros((n_frames, model.config.vocab_size), dtype=np.float32)
    chunk, pad = int(CHUNK_S * RATE), int(PAD_S * RATE)
    for begin in range(0, len(audio), chunk):
        lo, hi = max(0, begin - pad), min(len(audio), begin + chunk + pad)
        x = audio[lo:hi]
        x = (x - x.mean()) / (x.std() + 1e-7)  # processor do_normalize
        with torch.inference_mode():
            logits = model(torch.from_numpy(x)[None]).logits[0]
        lp = torch.log_softmax(logits, -1).numpy()
        f_lo, f_hi = (begin - lo) // hop, (min(begin + chunk, len(audio)) - lo) // hop
        dst = begin // hop
        n = min(f_hi - f_lo, n_frames - dst, len(lp) - f_lo)
        out[dst:dst + n] = lp[f_lo:f_lo + n]
    return out


def normalize(word, vocab):
    w = word.upper().replace("-", "")
    return [vocab[c] for c in w if c in vocab and c not in "|"]


def viterbi(lp, tokens, blank=0):
    """Standard CTC forced alignment. Returns (token_index, frame) for every frame."""
    T, L = lp.shape[0], len(tokens)
    S = 2 * L + 1
    ext = np.full(S, blank, dtype=np.int64)
    ext[1::2] = tokens
    skip = np.zeros(S, dtype=bool)  # may jump s-2 -> s (different consecutive labels)
    skip[3::2] = ext[3::2] != ext[1:-2:2]
    NEG = -1e30
    score = np.full(S, NEG, dtype=np.float64)
    score[0], score[1] = lp[0, blank], lp[0, ext[1]]
    back = np.zeros((T, S), dtype=np.int8)  # 0 stay, 1 from s-1, 2 from s-2
    for t in range(1, T):
        stay = score
        one = np.concatenate(([NEG], score[:-1]))
        two = np.where(skip, np.concatenate(([NEG, NEG], score[:-2])), NEG)
        stacked = np.stack([stay, one, two])
        choice = stacked.argmax(0)
        score = stacked[choice, np.arange(S)] + lp[t, ext]
        back[t] = choice
    s = S - 1 if score[S - 1] >= score[S - 2] else S - 2
    path = np.empty(T, dtype=np.int64)
    for t in range(T - 1, -1, -1):
        path[t] = s
        s -= int(back[t, s])
    return path, ext


def main():
    model_dir, wav_path, whisper_path, out_path = sys.argv[1:5]
    vocab = json.load(open(f"{model_dir}/vocab.json", encoding="utf-8"))
    model = Wav2Vec2ForCTC.from_pretrained(model_dir).eval()
    torch.set_num_threads(8)
    audio = load_wav(wav_path)
    t0 = time.time()
    lp = emissions(model, audio)
    t_emit = time.time() - t0

    words = json.load(open(whisper_path, encoding="utf-8"))["words"]
    tokens, owner = [], []  # owner[k] = word index of token k
    for i, w in enumerate(words):
        ids = normalize(w["text"], vocab)
        tokens += ids
        owner += [i] * len(ids)
    t0 = time.time()
    path, ext = viterbi(lp, np.array(tokens))
    t_align = time.time() - t0

    hop_s = model.config.inputs_to_logits_ratio / RATE
    first, last, conf = {}, {}, {}
    for t, s in enumerate(path):
        if s % 2 == 1:  # a label state, not blank
            k = s // 2
            i = owner[k]
            first.setdefault(i, t)
            last[i] = t
            conf.setdefault(i, []).append(float(np.exp(lp[t, ext[s]])))
    aligned = []
    for i, w in enumerate(words):
        if i in first:
            start, end = first[i] * hop_s, (last[i] + 1) * hop_s
            score = float(np.mean(conf[i]))
        else:  # no alignable characters (e.g. punctuation only): fall back to Whisper
            start, end, score = w["start"], w["end"], 0.0
        aligned.append({"text": w["text"], "start": round(start, 3), "end": round(end, 3),
                        "whisper_start": w["start"], "whisper_end": w["end"], "score": round(score, 3)})
    json.dump({"model": model_dir, "frame_seconds": hop_s, "emission_seconds": round(t_emit, 2),
               "align_seconds": round(t_align, 2), "words": aligned},
              open(out_path, "w", encoding="utf-8"), ensure_ascii=False, indent=1)
    print(f"emissions {t_emit:.1f}s, viterbi {t_align:.1f}s, {len(aligned)} words, {len(tokens)} tokens")


if __name__ == "__main__":
    main()
