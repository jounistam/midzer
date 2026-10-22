// Syncing live audio: the source reaches the player and midzer with different
// delays, which nobody knows in advance. The player taps along on the beat for
// a few bars; the engine judges each tap against the beat it tracked, and the
// median says how much later the player hears the source than midzer does.

/** Taps it takes. */
export const SYNC_TAPS = 12;
/** Taps further apart than this (median distance from their median, in ms) aren't steady enough to go by. */
const SCATTER_MS = 25;
/** The engine takes offsets up to a second either way. */
const MAX_OFFSET_MS = 1000;

/**
 * A tap's distance from the nearest beat, from how the engine judged it: `ms`
 * from sixteenth `sixteenth` at `bpm`. Null when it's nearer the eighth
 * between beats, which isn't a tap on the beat at any offset.
 */
export function offsetToBeat(ms: number, sixteenth: number, bpm: number): number | null {
  const step = 60_000 / bpm / 4;
  switch (sixteenth % 4) {
    case 0:
      return ms;
    case 1:
      return ms + step;
    case 3:
      return ms - step;
    default:
      return null;
  }
}

const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b);
  const mid = s.length >> 1;
  return s.length % 2 ? s[mid]! : (s[mid - 1]! + s[mid]!) / 2;
};

/**
 * The offset to use after taps that were `offsets` ms from the beat with
 * `old` in place: moved by their median. Null when there are too few taps
 * or they are too scattered.
 */
export function syncedOffset(old: number, offsets: number[]): number | null {
  if (offsets.length < SYNC_TAPS) return null;
  const mid = median(offsets);
  if (median(offsets.map((x) => Math.abs(x - mid))) > SCATTER_MS) return null;
  return Math.round(Math.max(-MAX_OFFSET_MS, Math.min(MAX_OFFSET_MS, old + mid)));
}
