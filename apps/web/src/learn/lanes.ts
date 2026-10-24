// Which targets the falling-notes view shows, and where, around the song
// position. Kept apart from the renderer so it can be tested.

import type { Target } from './targets';

export interface Placement {
  target: Target;
  /** Beats from the song position to the target's start; negative once it has passed. */
  offset: number;
  /** A copy shown for the next pass of a loop, so it is drawn as not played yet. */
  nextPass: boolean;
}

/** Bars `start..end` in beats. */
export interface BeatRange {
  start: number;
  end: number;
}

/**
 * Targets within `ahead` beats of `pos`, and those still sounding up to
 * `behind` beats ago. In a loop, the loop start's targets also come round
 * again after its end.
 */
export function placements(targets: readonly Target[], pos: number, ahead: number, behind: number, loop: BeatRange | null): Placement[] {
  const out: Placement[] = [];
  const visible = (t: Target, offset: number) => offset <= ahead && offset + t.len >= -behind;
  const inLoop = loop && pos >= loop.start - 1e-6 && pos < loop.end + 1e-6 ? loop : null;
  for (const target of targets) {
    if (inLoop && (target.beat < inLoop.start - 1e-6 || target.beat >= inLoop.end - 1e-6)) continue;
    const offset = target.beat - pos;
    if (visible(target, offset)) out.push({ target, offset, nextPass: false });
    if (inLoop) {
      const again = offset + (inLoop.end - inLoop.start);
      if (visible(target, again)) out.push({ target, offset: again, nextPass: true });
    }
  }
  return out;
}

/**
 * Song positions of the bar lines between `from` and `to`, in beats, from
 * where the bars start (`barStarts`); past the song's end, every four beats.
 */
export function barLines(from: number, to: number, starts: readonly number[] = [0]): number[] {
  const end = starts[starts.length - 1]!;
  const out = starts.filter((b) => b >= from && b <= to && b < end);
  for (let b = Math.max(end, end + Math.ceil((from - end) / 4) * 4); b <= to; b += 4) out.push(b);
  return out;
}
