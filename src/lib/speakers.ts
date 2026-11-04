// Speakers in a transcript: friendly default names, a short overview per speaker, and merging two
// speakers that turn out to be the same person.

export interface SpeakerUtterance {
  start: number;
  end: number;
  speaker?: string | null;
  text: string;
}

export interface SpeakerSummary {
  id: string;
  /** Number of utterances. */
  count: number;
  /** Total speaking time in seconds. */
  seconds: number;
  /** A representative stretch to listen to: the longest utterance, capped at `SAMPLE_MAX` s. */
  sample: { start: number; end: number; text: string };
}

const SAMPLE_MAX = 12;

/** "TALARE_3" → "Talare 3"; other ids (Jag, Mötet, names) are shown as they are. */
export function defaultSpeakerName(id: string): string {
  return id.startsWith("TALARE_") ? id.replace("TALARE_", "Talare ") : id;
}

/** `existing` names, plus a default name for every speaker in `utterances` that has none. */
export function withDefaultLabels(utterances: SpeakerUtterance[], existing: Record<string, string> = {}): Record<string, string> {
  const labels = { ...existing };
  for (const u of utterances) if (u.speaker && !labels[u.speaker]?.trim()) labels[u.speaker] = defaultSpeakerName(u.speaker);
  return labels;
}

/** One summary per speaker, in order of first appearance. */
export function summarizeSpeakers(utterances: SpeakerUtterance[]): SpeakerSummary[] {
  const by = new Map<string, SpeakerSummary>();
  for (const u of utterances) {
    if (!u.speaker) continue;
    const len = Math.max(0, u.end - u.start);
    const s = by.get(u.speaker) ?? { id: u.speaker, count: 0, seconds: 0, sample: { start: u.start, end: u.end, text: u.text } };
    s.count += 1;
    s.seconds += len;
    if (s.count === 1 || len > s.sample.end - s.sample.start) s.sample = { start: u.start, end: u.end, text: u.text };
    by.set(u.speaker, s);
  }
  return [...by.values()].map((s) => ({ ...s, sample: { ...s.sample, end: Math.min(s.sample.end, s.sample.start + SAMPLE_MAX) } }));
}

/** Give everything `from` said to `into`. */
export function mergeSpeakers<T extends SpeakerUtterance>(utterances: T[], from: string, into: string): T[] {
  if (from === into) return utterances;
  return utterances.map((u) => (u.speaker === from ? { ...u, speaker: into } : u));
}

/** True when the meeting side ("Mötet") is still one voice or has been split before. */
export function hasMeetingVoices(utterances: SpeakerUtterance[]): boolean {
  return utterances.some((u) => u.speaker === "Mötet" || !!u.speaker?.startsWith("TALARE_"));
}

/** How many people a participant list names, or null (let the app guess) when it is empty. */
export function countParticipants(names: string[]): number | null {
  const n = new Set(names.map((s) => s.trim().toLowerCase()).filter(Boolean)).size;
  return n > 0 ? n : null;
}
