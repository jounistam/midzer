import { describe, expect, it } from 'vitest';
import { barLines, placements } from './lanes';
import type { Target } from './targets';

const note = (id: number, beat: number, len = 1): Target => ({ id, kind: 'note', beat, len, keys: [60], pcs: [0] });
const targets = [note(0, 0), note(1, 2, 2), note(2, 4), note(3, 9)];

describe('falling-note placements', () => {
  it('shows what is ahead and what still sounds', () => {
    const ids = (pos: number) =>
      placements(targets, pos, 4, 0.5, null).map((p) => [p.target.id, Math.round(p.offset * 10) / 10]);
    expect(ids(0.4)).toEqual([[0, -0.4], [1, 1.6], [2, 3.6]]);
    expect(ids(1.6)).toEqual([[1, 0.4], [2, 2.4]]);
    expect(ids(3.9)).toEqual([[1, -1.9], [2, 0.1]]);
  });

  it('brings the loop start round again near the loop end', () => {
    const p = placements(targets, 7, 4, 0.5, { start: 0, end: 8 });
    expect(p.map((x) => [x.target.id, x.offset, x.nextPass])).toEqual([
      [0, 1, true],
      [1, 3, true],
    ]);
  });

  it('lists bar lines', () => {
    expect(barLines(2.5, 12)).toEqual([4, 8, 12]);
    // From the song's bar starts (4, 2 and 3 beats), then fours past its end.
    expect(barLines(1, 17, [0, 4, 6, 9])).toEqual([4, 6, 9, 13, 17]);
  });
});
