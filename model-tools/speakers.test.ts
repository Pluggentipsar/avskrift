import { test } from 'node:test';
import assert from 'node:assert/strict';
import { countParticipants, hasMeetingVoices, mergeSpeakers, summarizeSpeakers, withDefaultLabels } from '../src/lib/speakers.ts';

const u = (start: number, end: number, speaker: string | null, text = '') => ({ start, end, speaker, text });

test('new speakers get readable names and given names are kept', () => {
  const labels = withDefaultLabels([u(0, 1, 'Jag'), u(1, 2, 'TALARE_1'), u(2, 3, 'TALARE_2'), u(3, 4, null)], { TALARE_1: 'Kim', TALARE_2: ' ' });
  assert.deepEqual(labels, { Jag: 'Jag', TALARE_1: 'Kim', TALARE_2: 'Talare 2' });
});

test('each speaker gets time, count and the longest stretch to listen to', () => {
  const s = summarizeSpeakers([u(0, 2, 'TALARE_1', 'kort'), u(2, 5, 'Jag'), u(5, 40, 'TALARE_1', 'lång'), u(40, 41, 'TALARE_2')]);
  assert.deepEqual(s.map((x) => x.id), ['TALARE_1', 'Jag', 'TALARE_2']);
  assert.equal(s[0].count, 2);
  assert.equal(s[0].seconds, 37);
  assert.deepEqual(s[0].sample, { start: 5, end: 17, text: 'lång' }, 'capped at 12 s');
});

test('merging gives everything one speaker said to another', () => {
  const out = mergeSpeakers([u(0, 1, 'TALARE_1'), u(1, 2, 'TALARE_3'), u(2, 3, 'Jag')], 'TALARE_3', 'TALARE_1');
  assert.deepEqual(out.map((x) => x.speaker), ['TALARE_1', 'TALARE_1', 'Jag']);
});

test('meeting voices and participant counts', () => {
  assert.equal(hasMeetingVoices([u(0, 1, 'Jag'), u(1, 2, 'Mötet')]), true);
  assert.equal(hasMeetingVoices([u(0, 1, 'Jag')]), false);
  assert.equal(countParticipants(['Kim', ' kim ', 'Alex', '']), 2);
  assert.equal(countParticipants([]), null);
});
