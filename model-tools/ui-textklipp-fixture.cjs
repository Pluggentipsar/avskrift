// Build a Textklipp project fixture from local alignment output (never committed: it contains
// the user's own recording). Usage: node ui-textklipp-fixture.cjs WHISPER.json ALIGNED.json OUT.json
const fs = require('node:fs');
const [whisperPath, alignedPath, out] = process.argv.slice(2);
const whisper = JSON.parse(fs.readFileSync(whisperPath, 'utf8'));
const aligned = JSON.parse(fs.readFileSync(alignedPath, 'utf8'));
const words = aligned.words;
if (words.length !== whisper.words.length) throw Error('word count mismatch');
// One utterance per Whisper segment; words go to the segment their Whisper start falls in.
const utterances = whisper.segments.map(s => ({ start: s.start, end: s.end, speaker: null, text: s.text, words: [] }));
words.forEach((w, i) => {
  const ws = whisper.words[i].start;
  const seg = utterances.findLast(u => u.start <= ws + 1e-6) ?? utterances[0];
  seg.words.push({ start: w.start, end: w.end, text: w.text });
});
const kept = utterances.filter(u => u.words.length);
for (const u of kept) { u.start = u.words[0].start; u.end = u.words.at(-1).end; }
const project = {
  version: 1, id: 'tk-fixture', title: 'Testklipp', createdAt: '2026-10-02T10:00:00Z', updatedAt: '2026-10-02T10:00:00Z',
  sourcePath: 'C:/synthetic/test.mp4',
  media: { duration: aligned.audio_seconds, sizeBytes: 237858084, container: 'mov,mp4',
    video: { codec: 'h264', width: 1920, height: 1080, fps: 30, variableRate: false, rotation: 0 },
    audio: { codec: 'aac', sampleRate: 44100, channels: 2 } },
  status: 'ready', error: null,
  transcript: { utterances: kept, language: 'sv', model: 'kb-whisper-large', diarized: false },
  wordTimes: 'exakta',
  sounds: aligned.sounds.map((s, i) => ({ id: 1000000 + i, start: s.start, end: s.end, heard: s.heard, score: s.score })),
  pauses: aligned.pauses.map(p => [p.start, p.end]),
  proxy: 'proxy.mp4', edits: { deleted: [], removed: [], pauseLimit: null },
};
fs.writeFileSync(out, JSON.stringify(project));
console.log(`fixture: ${kept.length} paragraphs, ${words.length} words, ${project.sounds.length} sounds, ${project.pauses.length} pauses`);
