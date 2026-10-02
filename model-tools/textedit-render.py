"""Render a text-edited video: delete words/phrases from the transcript, cut the video to match.

Modes:
  whisper  naive cuts at Whisper word times, no fades (what a direct implementation gives)
  aligned  cuts at the quietest frame boundary between forced-aligned words + short audio fades

Usage: python textedit-render.py VIDEO AUDIO16K.wav ALIGNED.json EDITS.json OUT.mp4 whisper|aligned
EDITS.json: [{"phrase": "egentligen", "occurrence": 1}, {"from": "Jag heter", "from_occurrence": 1,
             "to": "säger det nu.", "to_occurrence": 1}, ...]  (occurrence is 1-based)
"""
import json
import re
import subprocess
import sys
import wave

import numpy as np

FPS_FALLBACK = 30.0
FADE_S = 0.010


def norm(s):
    return re.sub(r"[^\w]", "", s.lower())


def find(words, phrase, occurrence):
    target = [norm(p) for p in phrase.split()]
    keys = [norm(w["text"]) for w in words]
    hits = [i for i in range(len(keys) - len(target) + 1) if keys[i:i + len(target)] == target]
    if len(hits) < occurrence:
        raise SystemExit(f"phrase not found: {phrase!r} #{occurrence} (hits {len(hits)})")
    return hits[occurrence - 1], hits[occurrence - 1] + len(target) - 1


def deleted_ranges(words, edits):
    out = []
    for e in edits:
        if "phrase" in e and "within" in e:  # delete `phrase` inside a longer, unambiguous context
            a, b = find(words, e["within"], e.get("occurrence", 1))
            sa, sb = find(words[a:b + 1], e["phrase"], 1)
            out.append((a + sa, a + sb))
        elif "phrase" in e:
            out.append(find(words, e["phrase"], e.get("occurrence", 1)))
        else:
            a, _ = find(words, e["from"], e.get("from_occurrence", 1))
            _, b = find(words, e["to"], e.get("to_occurrence", 1))
            out.append((a, b))
    return sorted(out)


def probe_fps(video):
    r = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries", "stream=r_frame_rate",
                        "-of", "csv=p=0", video], capture_output=True, text=True).stdout.strip()
    try:
        n, d = r.split("/")
        return float(n) / float(d)
    except ValueError:
        return FPS_FALLBACK


def energy_db(wav_path):
    with wave.open(wav_path) as w:
        a = np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768
    rms = np.sqrt(np.convolve(a ** 2, np.ones(160) / 160, "same") + 1e-10)  # 10 ms window
    return 20 * np.log10(rms[::16])  # 1 ms resolution


def quiet_cut(db, fps, lo, hi):
    """Quietest video-frame boundary in [lo, hi]; nearest boundary to the quietest ms if none fits."""
    lo, hi = min(lo, hi), max(lo, hi)
    frames = np.arange(np.ceil(lo * fps), np.floor(hi * fps) + 1) / fps
    if len(frames) == 0:
        ms = np.arange(int(lo * 1000), max(int(hi * 1000), int(lo * 1000) + 1))
        best = ms[np.argmin(db[np.clip(ms, 0, len(db) - 1)])] / 1000
        return round(best * fps) / fps

    def loud(t):  # mean level +/-15 ms around the cut
        i = int(t * 1000)
        return db[max(0, i - 15):i + 16].mean()

    return min(frames, key=loud)


def cut_points(words, ranges, mode, db, fps, duration):
    cuts = []  # (cut_start, cut_end) removed intervals
    for a, b in ranges:
        if mode == "whisper":
            s, e = words[a]["whisper_start"], words[b]["whisper_end"]
        else:
            prev_end = words[a - 1]["end"] if a > 0 else 0.0
            next_start = words[b + 1]["start"] if b + 1 < len(words) else duration
            s = quiet_cut(db, fps, prev_end, words[a]["start"])
            e = quiet_cut(db, fps, words[b]["end"], next_start)
        cuts.append((s, e))
    keep, t = [], 0.0
    for s, e in cuts:
        if s > t:
            keep.append((t, s))
        t = max(t, e)
    if t < duration:
        keep.append((t, duration))
    return cuts, keep


def render(video, keep, mode, out):
    parts, labels = [], []
    for i, (s, e) in enumerate(keep):
        parts.append(f"[0:v]trim=start={s:.6f}:end={e:.6f},setpts=PTS-STARTPTS[v{i}]")
        a = f"[0:a]atrim=start={s:.6f}:end={e:.6f},asetpts=PTS-STARTPTS"
        if mode == "aligned":
            a += f",afade=t=in:d={FADE_S},afade=t=out:st={max(0.0, e - s - FADE_S):.6f}:d={FADE_S}"
        parts.append(a + f"[a{i}]")
        labels.append(f"[v{i}][a{i}]")
    graph = ";".join(parts) + ";" + "".join(labels) + f"concat=n={len(keep)}:v=1:a=1[v][a]"
    cmd = ["ffmpeg", "-v", "error", "-y", "-i", video, "-filter_complex", graph, "-map", "[v]", "-map", "[a]",
           "-c:v", "h264_nvenc", "-preset", "p5", "-cq", "20", "-c:a", "aac", "-b:a", "192k", out]
    subprocess.run(cmd, check=True)


def main():
    video, wav_path, aligned_path, edits_path, out, mode = sys.argv[1:7]
    words = json.load(open(aligned_path, encoding="utf-8"))["words"]
    edits = json.load(open(edits_path, encoding="utf-8-sig"))
    db = energy_db(wav_path)
    duration = len(db) / 1000
    fps = probe_fps(video)
    ranges = deleted_ranges(words, edits)
    cuts, keep = cut_points(words, ranges, mode, db, fps, duration)
    for (a, b), (s, e) in zip(ranges, cuts):
        text = " ".join(w["text"] for w in words[a:b + 1])
        print(f"cut {s:7.3f}-{e:7.3f}  {text[:70]}")
    render(video, keep, mode, out)
    kept = sum(e - s for s, e in keep)
    json.dump({"mode": mode, "fps": fps, "cuts": cuts, "keep": keep}, open(out + ".json", "w"), indent=1)
    print(f"{mode}: {len(keep)} pieces, {kept:.2f}s kept of {duration:.2f}s -> {out}")


if __name__ == "__main__":
    main()
