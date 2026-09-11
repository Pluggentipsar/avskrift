import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createSaveQueue } from '../src/lib/save-queue.ts';

test('a delayed older write finishes before a newer snapshot is written', async () => {
  const queue = createSaveQueue();
  const disk: string[] = [];
  let release!: () => void;
  const gate = new Promise<void>(resolve => release = resolve);
  const first = queue.enqueue(async () => { await gate; disk.push('old'); });
  const second = queue.enqueue(async () => { disk.push('new'); });
  await Promise.resolve();
  assert.deepEqual(disk, []);
  release();
  await Promise.all([first,second]);
  assert.deepEqual(disk,['old','new']);
});

test('failed storage remains retryable and drain waits for queued work', async () => {
  const queue = createSaveQueue();
  await assert.rejects(queue.enqueue(async () => { throw new Error('disk full'); }));
  let saved = false;
  void queue.enqueue(async () => { saved = true; });
  await queue.drain();
  assert.equal(saved, true);
});
