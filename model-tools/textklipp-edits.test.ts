import { test } from 'node:test';
import assert from 'node:assert/strict';
import { findRestarts, findAllRetakes, subtract, inKeep, type Token } from '../src/lib/textklipp/types.ts';

/** Words 0.4 s apart from `t0`. */
const say = (text: string, t0 = 0, id0 = 0): Token[] =>
  text.split(' ').map((w, i) => ({ kind: 'word', id: id0 + i, start: t0 + i * 0.4, end: t0 + i * 0.4 + 0.3, text: w }));
const words = (tokens: Token[], from: number, to: number) => tokens.slice(from, to + 1).map(t => t.text).join(' ');

test('a phrase restarted within a sentence suggests removing the first take', () => {
  const t = say('Jag tänkte att, jag tänkte att vi skulle börja med budgeten.');
  const [r] = findRestarts(t);
  assert.equal(r.kind, 'restart');
  assert.equal(words(t, r.from, r.to), 'Jag tänkte att,');
  assert.equal(t[r.laterAt].text, 'jag');
});

test('a filler between the takes and word endings still count as a restart', () => {
  const t = say('Hej och välkommen till, eh, hej och välkomna till den här kursen.');
  const [r] = findRestarts(t);
  assert.ok(r, 'found');
  assert.equal(words(t, r.from, r.to), 'Hej och välkommen till, eh,');
});

test('two words back to back count, two words further apart do not', () => {
  assert.equal(findRestarts(say('det är det är bra så')).length, 1);
  assert.equal(findRestarts(say('det är bra och det är fint')).length, 0);
});

test('ordinary speech without repeats gives no suggestions', () => {
  assert.equal(findRestarts(say('Vi går igenom budgeten och sedan schemat för nästa termin.')).length, 0);
});

test('restarts far apart in time are not restarts', () => {
  const t = [...say('jag tänkte att', 0, 0), ...say('jag tänkte att vi', 30, 10)];
  assert.equal(findRestarts(t).length, 0);
});

test('sentence retakes win over restarts inside them', () => {
  const t = say('Det här är en film om skolan. Det här är en film om skolan och AI.');
  const all = findAllRetakes(t);
  assert.equal(all.length, 1);
  assert.equal(all[0].kind, 'sentence');
});

test('subtracting a range splits, trims or keeps the others', () => {
  assert.deepEqual(subtract([[0, 10]], 3, 5), [[0, 3], [5, 10]]);
  assert.deepEqual(subtract([[0, 4], [6, 10]], 3, 7), [[0, 3], [7, 10]]);
  assert.deepEqual(subtract([[0, 2]], 3, 5), [[0, 2]]);
  assert.deepEqual(subtract([[3, 5]], 2, 6), []);
  assert.ok(inKeep([[0, 2], [5, 8]], 6) && !inKeep([[0, 2], [5, 8]], 3));
});
