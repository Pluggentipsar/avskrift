// Textklipp: types mirroring src-tauri/src/textklipp.rs and pure helpers for the editor.

export type Word = { start: number; end: number; text: string };
export type Utterance = { start: number; end: number; speaker: string | null; text: string; words: Word[] };
export type Transcript = { utterances: Utterance[]; language: string; model: string; diarized: boolean };
export type Sound = { id: number; start: number; end: number; heard: string; score: number };
export type EditList = { deleted: number[]; removed: [number, number][]; pauseLimit: number | null; kept: [number, number][] };
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
/** A cut edge: 'out' ends a kept range (a cut starts there), 'in' starts one (a cut ends). */
export type Edge = { kind: 'in' | 'out'; at: number };

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

/** m:ss,t – tenths of a second, for short cuts. */
export function fmtPrecise(seconds: number): string {
  const tenths = Math.floor((Math.max(0, seconds) % 1) * 10);
  return `${fmt(seconds)},${tenths}`;
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

// --- search -----------------------------------------------------------------------------------

const norm = (s: string) => s.toLocaleLowerCase('sv').replace(/[^\p{L}\p{N}]+/gu, '');

/** Token ranges (inclusive indices into `tokens`) whose words match `query` word by word;
 *  the last query word may be a prefix ("bibl" finds "biblioteket"). */
export function search(tokens: Token[], query: string): [number, number][] {
  const q = query.split(/\s+/).map(norm).filter(Boolean);
  if (!q.length) return [];
  const words = tokens.map((t, i) => ({ i, w: t.kind === 'word' ? norm(t.text) : '' })).filter(x => x.w);
  const out: [number, number][] = [];
  for (let s = 0; s + q.length <= words.length; s++) {
    const ok = q.every((part, k) => (k === q.length - 1 ? words[s + k].w.startsWith(part) : words[s + k].w === part));
    if (ok) out.push([words[s].i, words[s + q.length - 1].i]);
  }
  return out;
}

/** "1:23", "01:02:03", "83" or "83,5" → seconds; null when not a time. */
export function parseTime(s: string): number | null {
  const parts = s.trim().replace(',', '.').split(':');
  if (!parts.length || parts.length > 3 || parts.some(p => !/^\d+(\.\d+)?$/.test(p))) return null;
  return parts.reduce((acc, p) => acc * 60 + Number(p), 0);
}

// --- repeated takes ---------------------------------------------------------------------------

export type Retake = {
  /** A whole sentence said again, or a phrase restarted within a sentence ("jag tänkte att, jag tänkte att vi"). */
  kind: 'sentence' | 'restart';
  /** First and last token index (inclusive) of the earlier take(s) to remove. */
  from: number; to: number;
  /** Last token index of the earlier take itself (what is marked in the text). */
  earlierTo: number;
  /** Token index where the later take starts. */
  laterAt: number;
  earlier: string; later: string;
  start: number; end: number;
};

const RETAKE_WINDOW = 120; // seconds between takes
const RETAKE_MAX = 150;    // never suggest removing more than this
const RETAKE_PREFIX = 4;   // same opening words (3 catches phrases like "det här är") …
const RETAKE_SIMILAR = 0.6; // … or this share of word pairs, of the longer sentence

type Sentence = { from: number; to: number; words: string[]; start: number; text: string };

function sentences(tokens: Token[]): Sentence[] {
  const out: Sentence[] = [];
  let cur: Sentence | null = null;
  tokens.forEach((t, i) => {
    if (t.kind !== 'word') return;
    if (!cur) cur = { from: i, to: i, words: [], start: t.start, text: '' };
    cur.to = i; cur.words.push(norm(t.text)); cur.text += (cur.text ? ' ' : '') + t.text;
    if (/[.!?…]$/.test(t.text) || cur.words.length >= 40) { out.push(cur); cur = null; }
  });
  if (cur) out.push(cur);
  return out.filter(s => s.words.length >= RETAKE_PREFIX);
}

function pairs(words: string[]): Set<string> {
  const s = new Set<string>();
  for (let i = 0; i + 1 < words.length; i++) s.add(words[i] + ' ' + words[i + 1]);
  return s;
}

/** Earlier takes of something said again shortly after (restarts, "nej, en gång till").
 *  Each suggestion removes from the start of an earlier take up to the later one, so accepting
 *  every suggestion keeps the last take. */
export function findRetakes(tokens: Token[]): Retake[] {
  const ss = sentences(tokens);
  const out: Retake[] = [];
  for (let i = 0; i < ss.length; i++) {
    const a = ss[i], pa = pairs(a.words);
    for (let j = i + 1; j < ss.length && ss[j].start - a.start <= RETAKE_WINDOW; j++) {
      const b = ss[j];
      const samePrefix = a.words.slice(0, RETAKE_PREFIX).join(' ') === b.words.slice(0, RETAKE_PREFIX).join(' ');
      const pb = pairs(b.words);
      const shared = [...pa].filter(p => pb.has(p)).length / Math.max(1, pa.size, pb.size);
      if (!samePrefix && shared < RETAKE_SIMILAR) continue;
      const end = tokens[b.from - 1]?.end ?? b.start;
      if (end - a.start > RETAKE_MAX) break;
      if (!out.some(r => r.from <= a.from && a.from <= r.to)) {
        out.push({ kind: 'sentence', from: a.from, to: b.from - 1, earlierTo: a.to, laterAt: b.from, earlier: a.text, later: b.text, start: a.start, end });
      }
      break;
    }
  }
  return out;
}

// --- restarts within a sentence ------------------------------------------------------------

const RESTART_LONGEST = 6;   // words in a repeated phrase, longest first
const RESTART_GAP = 3;       // words allowed between the two takes ("jag tänkte, eh, jag tänkte")
const RESTART_SECONDS = 10;  // time between the end of the first take and the second
const RESTART_WORDS = 12;    // a longer first take is a sentence of its own, not a false start
/** Words that may stand between an abandoned take and the new one. */
const FILLERS = new Set(['eh', 'öh', 'äh', 'ehm', 'öhm', 'hm', 'hmm', 'mm', 'alltså', 'asså', 'nej', 'förlåt', 'vänta', 'ja', 'okej']);

/** Words compared loosely: case and punctuation ignored, and long words by their first five
 *  letters, so "välkommen" and "välkomna" count as the same word. */
const loose = (s: string) => { const w = norm(s); return w.length > 5 ? w.slice(0, 5) : w; };

/** Phrases started again right away: the same 3–6 words (or 2 words back to back) repeated after at
 *  most a few filler words. Only abandoned takes count: one that ends as a finished sentence is a
 *  deliberate repetition ("Det här är min ingång. Det här är mitt patos."). The earlier take is
 *  suggested for removal, keeping the later one. */
export function findRestarts(tokens: Token[]): Retake[] {
  const words = tokens.map((t, i) => ({ t, i })).filter(w => w.t.kind === 'word');
  const keys = words.map(w => loose(w.t.text));
  const out: Retake[] = [];
  const same = (a: number, b: number, n: number) => {
    for (let k = 0; k < n; k++) if (!keys[a + k] || keys[a + k] !== keys[b + k]) return false;
    return true;
  };
  for (let i = 0; i < words.length; ) {
    let found: Retake | null = null;
    for (let n = RESTART_LONGEST; n >= 2 && !found; n--) {
      for (let gap = 0; gap <= (n >= 3 ? RESTART_GAP : 0) && !found; gap++) {
        const j = i + n + gap;
        if (j + n > words.length || !same(i, j, n)) continue;
        if (words[j].t.start - words[i + n - 1].t.end > RESTART_SECONDS) continue;
        if (j - i > RESTART_WORDS) continue;
        if (keys.slice(i + n, j).some(k => !FILLERS.has(k))) continue;
        const last = words[j - 1].t.text.trim();
        if (/[.!?]$/.test(last) && !/(\.\.\.|…)$/.test(last)) continue;
        const from = words[i].i, laterAt = words[j].i;
        found = {
          kind: 'restart', from, to: laterAt - 1, earlierTo: laterAt - 1, laterAt,
          earlier: tokens.slice(from, laterAt).map(t => t.text).join(' '),
          later: words.slice(j, j + n).map(w => w.t.text).join(' '),
          start: tokens[from].start, end: tokens[laterAt - 1].end,
        };
      }
    }
    if (found) { out.push(found); i = words.findIndex(w => w.i === found!.laterAt); }
    else i++;
  }
  return out;
}

/** Sentence retakes and restarts together, in time order, without overlaps (sentences win). */
export function findAllRetakes(tokens: Token[]): Retake[] {
  const sentences = findRetakes(tokens);
  const restarts = findRestarts(tokens).filter(r => !sentences.some(s => r.from <= s.to && s.from <= r.to));
  return [...sentences, ...restarts].sort((a, b) => a.start - b.start);
}

// --- time ranges ------------------------------------------------------------------------------

/** Whether source time `t` survives the edit. */
export function inKeep(keep: [number, number][], t: number): boolean {
  return keep.some(([a, b]) => t >= a && t <= b);
}

/** `ranges` with `[a, b]` taken out of each. */
export function subtract(ranges: [number, number][], a: number, b: number): [number, number][] {
  const out: [number, number][] = [];
  for (const [x, y] of ranges) {
    if (y <= a || x >= b) { out.push([x, y]); continue; }
    if (x < a) out.push([x, a]);
    if (y > b) out.push([b, y]);
  }
  return out;
}
