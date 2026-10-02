"""Check an exported Textklipp film against its source audio, all the way through.

For points along the edited timeline, a short window of the exported audio is located in the
source audio by cross-correlation and compared with where the keep ranges say it must come from.
Reports the error at each point; a drift would show up as a growing error.

Usage: python textklipp-sync-check.py EXPORT.mp4 SOURCE16K.wav KEEP.json
"""
import json
import subprocess
import sys
import wave

import numpy as np

RATE = 16000
WIN = 0.5      # seconds of exported audio per probe
SEARCH = 0.25  # +- seconds searched around the expected source position


def wav(path):
    with wave.open(path) as w:
        return np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768


def decode(path):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-vn", "-ac", "1", "-ar", str(RATE), "-f", "s16le", "-"],
                         capture_output=True, check=True).stdout
    return np.frombuffer(raw, np.int16).astype(np.float32) / 32768


def to_source(keep, e):
    off = 0.0
    for a, b in keep:
        if e <= off + (b - a):
            return a + (e - off), (b - a) - (e - off)
        off += b - a
    return None, 0


def main():
    out, src_path, keep_path = sys.argv[1:4]
    exp, src, keep = decode(out), wav(src_path), json.load(open(keep_path))
    total = sum(b - a for a, b in keep)
    errors = []
    for e in np.arange(1.0, total - 1.0, 2.0):
        s, room = to_source(keep, e)
        if s is None or room < WIN + 0.05:
            continue  # window would straddle a cut
        x = exp[int(e * RATE): int((e + WIN) * RATE)]
        if np.sqrt(np.mean(x ** 2)) < 0.01:
            continue  # silence cannot be located
        lo = int((s - SEARCH) * RATE)
        y = src[max(0, lo): int((s + WIN + SEARCH) * RATE)]
        n = len(x) + len(y)
        corr = np.fft.irfft(np.fft.rfft(y, n) * np.conj(np.fft.rfft(x, n)), n)[: len(y) - len(x) + 1]
        found = (max(0, lo) + int(np.argmax(corr))) / RATE
        errors.append((e, (found - s) * 1000))
    errs = np.array([v for _, v in errors])
    print(f"{len(errors)} probes over {total:.1f} s edited: error median {np.median(errs):+.1f} ms, "
          f"min {errs.min():+.1f}, max {errs.max():+.1f}, first {errs[0]:+.1f}, last {errs[-1]:+.1f}")
    import os
    if os.environ.get('AVSKRIFT_SYNC_VERBOSE'):
        print(' '.join(f'{t:.0f}:{v:+.0f}' for t, v in errors))
    worst = sorted(errors, key=lambda t: -abs(t[1]))[:5]
    print("worst:", ", ".join(f"{t:.0f}s {v:+.1f}ms" for t, v in worst))


if __name__ == "__main__":
    main()
