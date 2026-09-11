/** Serialize immutable job snapshots so a slow older write cannot replace a newer one. */
export function createSaveQueue() {
  let tail: Promise<unknown> = Promise.resolve();
  return {
    enqueue<T>(write: () => Promise<T>): Promise<T> {
      const result = tail.then(write);
      tail = result.catch(() => undefined);
      return result;
    },
    async drain() { await tail; },
  };
}
