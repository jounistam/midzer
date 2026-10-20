/**
 * Median offset (ms) of `taps` from the nearest heard beat in `beats`, or
 * null when the taps are too scattered to trust. Positive means late.
 */
export function tapOffset(taps: number[], beats: number[]): number | null {
  if (beats.length < 2) return null;
  const interval = (beats[beats.length - 1]! - beats[0]!) / (beats.length - 1);
  const offsets = taps
    .map((t) => {
      let nearest = beats[0]!;
      for (const b of beats) if (Math.abs(t - b) < Math.abs(t - nearest)) nearest = b;
      return t - nearest;
    })
    // A tap more than a third of a beat away is a guess, not a tap with the click.
    .filter((o) => Math.abs(o) < interval / 3)
    .sort((a, b) => a - b);
  if (offsets.length < Math.ceil(taps.length * 0.6)) return null;
  const mid = offsets.length >> 1;
  const median = offsets.length % 2 ? offsets[mid]! : (offsets[mid - 1]! + offsets[mid]!) / 2;
  const spread = offsets[offsets.length - 1]! - offsets[0]!;
  return spread > interval / 2 ? null : median;
}
