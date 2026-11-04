// Time left for a long job from its progress so far. Progress is reported as 0–100 by the engines
// (Whisper, Pianissimo, meeting completion, Textklipp import and export).

const SETTLE_MS = 5000; // no estimate in the first seconds: model loading makes the start uneven
const MIN_PCT = 2;

/** Tracks one job. `update` returns seconds left, or null while there is no fair estimate yet.
 *  Progress going backwards (a new pass, a retry on CPU) starts the estimate over. */
export function createEta() {
  let start: number | null = null, last = -1, smooth: number | null = null, pausedAt: number | null = null;
  return {
    /** `paused`: the job is on hold; the estimate stands still and the pause is not counted. */
    update(pct: number | null | undefined, now = Date.now(), paused = false): number | null {
      if (pct == null || !isFinite(pct)) { start = null; last = -1; smooth = null; pausedAt = null; return null; }
      if (paused) { pausedAt ??= now; return smooth; }
      if (pausedAt !== null) { if (start !== null) start += now - pausedAt; pausedAt = null; }
      if (start === null || pct < last) { start = now; smooth = null; }
      last = pct;
      const elapsed = now - start;
      if (pct < MIN_PCT || pct >= 100 || elapsed < SETTLE_MS) return pct >= 100 ? 0 : null;
      const raw = (elapsed / 1000) * (100 - pct) / pct;
      // Steady the number: follow the raw estimate, but not every wobble.
      smooth = smooth === null ? raw : smooth * 0.7 + raw * 0.3;
      return smooth;
    },
  };
}

/** "ca 4 min kvar", rounded so the number does not flicker. */
export function fmtLeft(seconds: number | null): string {
  if (seconds === null) return 'räknar ut tid kvar…';
  if (seconds < 45) return 'under en minut kvar';
  const min = Math.round(seconds / 60);
  if (min < 60) return `ca ${min} min kvar`;
  const h = Math.floor(min / 60), m = Math.round((min % 60) / 5) * 5;
  return m ? `ca ${h} h ${m} min kvar` : `ca ${h} h kvar`;
}
