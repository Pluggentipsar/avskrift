"""How much does the sound dip at each join of an exported Textklipp film?

For every join (from the exported pieces, KEEP.json), compares the quietest 5 ms within +-25 ms
of the join with the typical level 50-200 ms on either side. A fade to digital silence shows as a
deep dip; a room-tone crossfade should leave the room's noise in place (little or no dip).

Usage: python textklipp-join-check.py EXPORT.mp4 KEEP.json
"""
import json
import subprocess
import sys

import numpy as np

RATE = 48000


def decode(path):
    raw = subprocess.run(["ffmpeg", "-v", "error", "-i", path, "-vn", "-ac", "1", "-ar", str(RATE), "-f", "s16le", "-"],
                         capture_output=True, check=True).stdout
    return np.frombuffer(raw, np.int16).astype(np.float32) / 32768


def db(x):
    return 20 * np.log10(np.sqrt(np.mean(x ** 2)) + 1e-9)


def main():
    audio, keep = decode(sys.argv[1]), json.load(open(sys.argv[2]))
    joins, t = [], 0.0
    for a, b in keep[:-1]:
        t += b - a
        joins.append(t)
    w = int(0.005 * RATE)
    dips, quiet_dips = [], []
    for j in joins:
        c = int(j * RATE)
        near = [audio[i:i + w] for i in range(c - int(0.025 * RATE), c + int(0.025 * RATE) - w, w // 2)]
        around = np.concatenate([audio[c - int(0.2 * RATE):c - int(0.05 * RATE)], audio[c + int(0.05 * RATE):c + int(0.2 * RATE)]])
        if len(around) < RATE * 0.2 or not near:
            continue
        level = db(around)
        dip = level - min(db(x) for x in near)
        dips.append(dip)
        if level < -45:  # a join inside a pause (room tone, no speech around it)
            quiet_dips.append(dip)
            if __import__('os').environ.get('JOIN_VERBOSE'):
                before, after = db(audio[c - int(0.05 * RATE):c - int(0.03 * RATE)]), db(audio[c + int(0.03 * RATE):c + int(0.05 * RATE)])
                print(f'  join {j:7.3f}s level {level:6.1f} dB, dip {dip:5.1f} dB, 30-50 ms before {before:6.1f}, after {after:6.1f}')
    d, q = np.array(dips), np.array(quiet_dips)
    print(f"{len(d)} joins: dip median {np.median(d):.1f} dB, p90 {np.percentile(d, 90):.1f} dB, max {d.max():.1f} dB")
    if len(q):
        print(f"{len(q)} joins in pauses: dip median {np.median(q):.1f} dB, p90 {np.percentile(q, 90):.1f} dB, "
              f"> 10 dB: {int((q > 10).sum())}")


if __name__ == "__main__":
    main()
