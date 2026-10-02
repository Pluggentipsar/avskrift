// Textklipp: types mirroring src-tauri/src/textklipp.rs and pure helpers for the editor.

export type Word = { start: number; end: number; text: string };
export type Utterance = { start: number; end: number; speaker: string | null; text: string; words: Word[] };
export type Transcript = { utterances: Utterance[]; language: string; model: string; diarized: boolean };
export type Sound = { id: number; start: number; end: number; heard: string; score: number };
export type EditList = { deleted: number[]; removed: [number, number][]; pauseLimit: number | null };
export type MediaInfo = {
  duration: number; sizeBytes: number; container: string;
  video: { codec: string; width: number; height: number; fps: number; variableRate: boolean; rotation: number } | null;
  audio: { codec: string; sampleRate: number; channels: number } | null;
};
export type Project = {
  version: number; id: string; title: string; createdAt: string; updatedAt: string; sourcePath: string;
  media: MediaInfo; status: 'importing' | 'ready' | 'failed'; error: string | null; transcript: Transcript | null;
  wordTimes: string; sounds: Sound[]; pauses: [number, number][]; proxy: string | null; edits: EditList;
};
export type ProjectMeta = { id: string; title: string; updatedAt: string; duration: number; status: Project['status']; hasProxy: boolean };
export type Preview = { keep: [number, number][]; editedDuration: number };
export type Probe = { media: MediaInfo; workingBytes: number };

/** One clickable unit in the document: a transcript word, or speech the transcript lacks. */
export type Token = { kind: 'word' | 'sound'; id: number; start: number; end: number; text: string };
export type Paragraph = { key: number; speaker: string | null; start: number; tokens: Token[] };

/** Readings of sound blocks below this confidence are shown only as "ljud". */
export const SOUND_READABLE = 0.8;
/** A new paragraph starts after this much silence, on a speaker change, or past this many words. */
const PARAGRAPH_GAP = 1.2, PARAGRAPH_WORDS = 90;

/** Reading paragraphs: utterances joined while the same speaker keeps talking. Word ids are their
 *  index over all words (as in the backend). Sound blocks go where their time puts them. */
export function paragraphs(p: Project): Paragraph[] {
  const out: Paragraph[] = [];
  let id = 0, lastEnd = -Infinity;
  for (const u of p.transcript?.utterances ?? []) {
    const tokens: Token[] = u.words.map(w => ({ kind: 'word', id: id++, start: w.start, end: w.end, text: w.text }));
    if (!tokens.length) continue;
    const prev = out[out.length - 1];
    if (prev && prev.speaker === u.speaker && tokens[0].start - lastEnd < PARAGRAPH_GAP && prev.tokens.length < PARAGRAPH_WORDS) prev.tokens.push(...tokens);
    else out.push({ key: out.length, speaker: u.speaker, start: tokens[0].start, tokens });
    lastEnd = tokens[tokens.length - 1].end;
  }
  for (const s of p.sounds) {
    const text = (s.score ?? 0) >= SOUND_READABLE ? s.heard.toLocaleLowerCase('sv') : 'ljud';
    const sound: Token = { kind: 'sound', id: s.id, start: s.start, end: s.end, text };
    const target = out.find((q, i) => s.start < (out[i + 1]?.start ?? Infinity)) ?? out[out.length - 1];
    if (!target) continue;
    const at = target.tokens.findIndex(t => t.start > s.start);
    target.tokens.splice(at < 0 ? target.tokens.length : at, 0, sound);
  }
  return out;
}

/** Where playback should be at source time `t`: `t` itself, the start of the next kept range,
 *  or null when nothing is left after it. */
export function playable(keep: [number, number][], t: number): number | null {
  for (const [a, b] of keep) {
    if (t < a) return a;
    if (t < b - 0.01) return t;
  }
  return null;
}

/** Source time → edited timeline time (start of the next kept range when `t` was cut). */
export function toEdited(keep: [number, number][], t: number): number {
  let offset = 0;
  for (const [a, b] of keep) {
    if (t < a) return offset;
    if (t <= b) return offset + t - a;
    offset += b - a;
  }
  return offset;
}

/** Edited timeline time → source time. */
export function toSource(keep: [number, number][], e: number): number {
  let offset = 0;
  for (const [a, b] of keep) {
    if (e <= offset + (b - a)) return a + (e - offset);
    offset += b - a;
  }
  return keep.length ? keep[keep.length - 1][1] : 0;
}

export function fmt(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = s % 60;
  const two = (n: number) => String(n).padStart(2, '0');
  return h ? `${h}:${two(m)}:${two(r)}` : `${m}:${two(r)}`;
}

export function bytes(n: number): string {
  return n >= 1e9 ? `${(n / 1e9).toLocaleString('sv-SE', { maximumFractionDigits: 1 })} GB` : `${Math.round(n / 1e6)} MB`;
}

/** Binary search: index of the last token starting at or before `t` (tokens sorted by start). */
export function tokenAt(sorted: Token[], t: number): number {
  let lo = 0, hi = sorted.length - 1, found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (sorted[mid].start <= t) { found = mid; lo = mid + 1; } else hi = mid - 1;
  }
  return found;
}
