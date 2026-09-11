export type Word = { start: number; end: number; text: string };
export type Utterance = { start: number; end: number; speaker: string | null; text: string; words?: Word[] };

/** Bound mounted word controls even when an entire meeting has the same speaker. */
export function transcriptBlocks(utterances: Utterance[]) {
  const blocks: { start: number; end: number; characters: number }[] = [];
  let start = 0, words = 0, characters = 0;
  utterances.forEach((u, i) => {
    const count = u.words?.length || Math.ceil(u.text.length / 6);
    if (i > start && (i - start >= 16 || words + count > 350)) {
      blocks.push({ start, end: i, characters }); start = i; words = characters = 0;
    }
    words += count; characters += u.text.length;
  });
  if (start < utterances.length) blocks.push({ start, end: utterances.length, characters });
  return blocks;
}

/** Prefix maxima preserve the first matching segment when speaker tracks overlap. */
export function playbackIndex(utterances: Utterance[]) {
  let max = -Infinity;
  const ends = utterances.map(u => (max = Math.max(max, u.end)));
  const sorted = utterances.every((u, i) => !i || u.start >= utterances[i - 1].start);
  return (time: number) => {
    if (!sorted) return utterances.findIndex(u => u.start <= time && time < u.end);
    let lo = 0, hi = ends.length;
    while (lo < hi) { const mid = (lo + hi) >>> 1; if (ends[mid] <= time) lo = mid + 1; else hi = mid; }
    for (let i = lo; i < utterances.length && utterances[i].start <= time; i++) {
      if (time < utterances[i].end) return i;
    }
    return -1;
  };
}

export function matchingSegments(texts: string[], query: string) {
  const term = query.trim().toLocaleLowerCase('sv');
  return term ? texts.flatMap((text, i) => text.includes(term) ? [i] : []) : [];
}
