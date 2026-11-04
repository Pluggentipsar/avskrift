import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createEta, fmtLeft } from '../src/lib/eta.ts';

test('steady progress gives the remaining time', () => {
  const eta = createEta();
  eta.update(0, 0);
  // 20 % after 60 s → about 240 s left.
  assert.equal(eta.update(20, 60_000), 240);
});

test('no estimate in the first seconds or before a little progress', () => {
  const eta = createEta();
  eta.update(0, 0);
  assert.equal(eta.update(50, 3_000), null);
  assert.equal(createEta().update(1, 60_000), null);
});

test('progress going backwards (a new pass) starts over', () => {
  const eta = createEta();
  eta.update(0, 0);
  eta.update(90, 90_000);
  assert.equal(eta.update(5, 100_000), null, 'just restarted');
  assert.ok(Math.abs(eta.update(10, 110_000)! - 90) < 1e-9, '10 s for 5 % → 90 s left');
});

test('a pause stands still and is not counted afterwards', () => {
  const eta = createEta();
  eta.update(0, 0);
  const before = eta.update(20, 60_000);
  assert.equal(eta.update(20, 60_000, true), before, 'paused right after the last report');
  assert.equal(eta.update(20, 600_000, true), before, 'unchanged while paused');
  // Resumed after a 9.5 min pause: 30 % after 60 s of work → 140 s left; smoothed from 240 → 210.
  const after = eta.update(30, 630_000)!;
  assert.ok(Math.abs(after - 210) < 1e-9, `pause not counted, got ${after}`);
});

test('time left reads naturally', () => {
  assert.equal(fmtLeft(null), 'räknar ut tid kvar…');
  assert.equal(fmtLeft(30), 'under en minut kvar');
  assert.equal(fmtLeft(250), 'ca 4 min kvar');
  assert.equal(fmtLeft(3600 + 22 * 60), 'ca 1 h 20 min kvar');
  assert.equal(fmtLeft(7200), 'ca 2 h kvar');
});
