import { describe, expect, it } from 'vitest';
import { parseChart, type Bar } from './song';
import {
  barCutAt,
  canJoin,
  changeTempo,
  chartFixes,
  chordBlocks,
  evenOut,
  fromTaps,
  keepFixes,
  moveChordEdge,
  notAOne,
  nudge,
  oneIsHere,
  onsetNear,
  setChord,
  songChords,
  type Timeline,
} from './timeline';

const lengths = (bars: Bar[]) => bars.map((b) => b.beats.length);
/** A steady beat map: `n` beats half a second apart, plus the end. */
const steady = (n: number, from = 1) => Array.from({ length: n + 1 }, (_, i) => from + i * 0.5);
const timeline = (chart: string, fixes = {}): Timeline => {
  const bars = parseChart(chart).bars;
  return { bars, beatTimes: steady(bars.reduce((n, b) => n + b.beats.length, 0)), fixes };
};

describe('bar lines', () => {
  it('"The 1 is here" ends the bar short and lays the bars after it again', () => {
    const t = timeline('C | F | G | Am | [Chorus] F | G');
    expect(barCutAt(t.bars, 3)).toBe(3);
    const next = oneIsHere(t, 3);
    expect(lengths(next.bars)).toEqual([3, 4, 4, 4, 4, 4, 1]);
    expect(next.fixes.bars).toEqual([3]);
    // Chords stay on their beats, the section on its beat.
    expect(next.bars[1]!.beats).toEqual(['', 'F', '', '']);
    expect(next.bars[4]!.section).toBe('B');
    expect(next.bars[4]!.beats).toEqual(['', 'F', '', '']);
    expect(next.beatTimes).toBe(t.beatTimes);
  });

  it('lays bars again only up to the next bar line set by hand', () => {
    const t = oneIsHere(timeline('C | F | G | Am | F | G'), 12);
    expect(lengths(t.bars)).toEqual([4, 4, 4, 4, 4, 4]);
    const next = oneIsHere(t, 5);
    expect(lengths(next.bars)).toEqual([4, 1, 4, 3, 4, 4, 4]);
    expect(next.fixes.bars).toEqual([5, 12]);
  });

  it('"Not a 1" joins a bar to the one before, if they fit', () => {
    const t = timeline('C | [2/4] F | [4/4] G | Am');
    expect(canJoin(t.bars, 4)).toBe(true);
    expect(canJoin(t.bars, 10)).toBe(false);
    expect(lengths(notAOne(t, 4).bars)).toEqual([6, 4, 4]);
    expect(notAOne(t, 10)).toBe(t);
    expect(notAOne(oneIsHere(t, 2), 2).fixes.bars).toEqual([]);
  });
});

describe('beat times', () => {
  it('nudges a beat or a selection, not past the beats around it', () => {
    const t = timeline('C | F');
    const next = nudge(t, 2, 3, 0.01);
    expect(next.beatTimes.slice(1, 5).map((s) => Math.round(s * 1000) / 1000)).toEqual([1.5, 2.01, 2.51, 3]);
    expect(next.fixes.beats).toBe(true);
    expect(nudge(t, 2, 2, 5).beatTimes[2]).toBeCloseTo(2.45);
    expect(nudge(t, 0, 0, -5).beatTimes[0]).toBe(0);
  });

  it('evens out a selection between its first and last beats', () => {
    const t = nudge(timeline('C | F'), 2, 2, 0.2);
    expect(evenOut(t, 0, 4).beatTimes.slice(0, 5)).toEqual([1, 1.5, 2, 2.5, 3]);
  });

  it('finds a rise in the level near a moment', () => {
    const env = new Uint8Array(300).fill(40);
    env.fill(200, 152);
    expect(onsetNear(env, 1.5)).toBe(1.52);
    expect(onsetNear(env, 2.5)).toBeNull();
  });

  it('doubles and halves the tempo with the chords kept in time', () => {
    const t = timeline('C | F G');
    const double = changeTempo(t, 2);
    expect(double.beatTimes).toEqual(steady(16, 1).map((s) => 1 + (s - 1) / 2));
    expect(lengths(double.bars)).toEqual([4, 4, 4, 4]);
    expect(double.bars.map((b) => b.beats[0])).toEqual(['C', '', 'F', 'G']);
    const half = changeTempo(t, 0.5);
    expect(half.beatTimes).toEqual([1, 2, 3, 4, 5]);
    expect(half.bars.map((b) => b.beats)).toEqual([['C', '', 'F', 'G']]);
    // Twice then half is where it started.
    expect(changeTempo(double, 0.5).bars).toEqual(t.bars);
  });

  it('changes the tempo of a selection only, the bars after it moving with it', () => {
    const t = timeline('C | F | G');
    const next = changeTempo(t, 0.5, 4, 8);
    expect(next.beatTimes.length).toBe(11);
    expect(lengths(next.bars)).toEqual([4, 2, 4]);
    expect(next.bars[2]!.beats[0]).toBe('G');
  });

  it('takes the beats from taps, filling a missed one', () => {
    const t = timeline('C | F');
    // Taps a little late, with the fourth beat's tap missing.
    const next = fromTaps(t, 0, 7, [1.1, 1.6, 2.1, 3.1, 3.6, 4.1]);
    expect(next.beatTimes.slice(0, 8).map((s) => Math.round(s * 100) / 100)).toEqual([1.1, 1.6, 2.1, 2.6, 3.1, 3.6, 4.1, 4.6]);
    expect(fromTaps(t, 0, 7, [2])).toBe(t);
  });
});

describe('chords', () => {
  it('draws one block per chord held', () => {
    expect(chordBlocks(parseChart('C | C | F G').bars)).toEqual([
      { start: 0, end: 8, name: 'C' },
      { start: 8, end: 10, name: 'F' },
      { start: 10, end: 12, name: 'G' },
    ]);
  });

  it('types a chord for a block', () => {
    const t = timeline('C | F');
    const [, f] = chordBlocks(t.bars);
    const next = setChord(t, f!, 'Dm');
    expect(next.bars[1]!.beats).toEqual(['Dm', '', '', '']);
    expect(next.fixes.chords).toEqual([4, 5, 6, 7]);
    expect(setChord(t, f!, null).bars[1]!.beats[0]).toBe('N.C.');
  });

  it('moves a change of chord either way, between its neighbours', () => {
    const t = timeline('C | F | G');
    expect(moveChordEdge(t, 4, 6).bars.map((b) => b.beats)).toEqual([['C', '', '', ''], ['', '', 'F', ''], ['G', '', '', '']]);
    expect(moveChordEdge(t, 4, 2).bars[0]!.beats).toEqual(['C', '', 'F', '']);
    expect(moveChordEdge(t, 4, 20).bars[1]!.beats).toEqual(['', '', '', 'F']);
    expect(moveChordEdge(t, 4, 6).fixes.chords).toEqual([4, 5, 6]);
  });

  it('lists the song\'s chords, most used first', () => {
    expect(songChords(parseChart('Am | Am | F G | C').bars)).toEqual(['Am', 'C', 'F', 'G']);
  });
});

describe('fixes', () => {
  it('records hand edits in the chart grid', () => {
    const before = parseChart('C | F').bars;
    const after = parseChart('C | [3/4] Dm | [1/4] G').bars;
    expect(chartFixes(before, after)).toEqual({ chords: [4, 5, 6, 7], bars: [7] });
  });

  it('keeps fixed beats and bars, with the new chords moved on by time', () => {
    const old = oneIsHere(nudge(timeline('C | F | G'), 4, 4, 0.1), 3);
    const fresh = { bars: parseChart('Am | Dm | E').bars, beatTimes: steady(12) };
    const kept = keepFixes(old, fresh);
    expect(kept.beatTimes).toEqual(old.beatTimes);
    expect(lengths(kept.bars)).toEqual(lengths(old.bars));
    // Beat 3 (2.5–3.1 s) is mostly in the new first bar, Am; beat 4 in the second, Dm.
    expect(kept.bars.map((b) => b.beats)[1]).toEqual(['', 'Dm', '', '']);
  });

  it('takes new beats and bars, keeping lines and chords set by hand', () => {
    const lined = oneIsHere(timeline('C | F | G | C'), 6);
    const old = setChord(lined, chordBlocks(lined.bars)[1]!, 'D');
    // The new analysis has the 1s where they were before the fix.
    const fresh = { bars: parseChart('C | F | G | C').bars, beatTimes: steady(16, 1.02) };
    const kept = keepFixes(old, fresh);
    expect(kept.beatTimes).toEqual(fresh.beatTimes);
    expect(lengths(kept.bars)).toEqual([4, 2, 4, 4, 2]);
    expect(kept.bars[1]!.beats).toEqual(['D', '']);
    expect(kept.bars[2]!.beats).toEqual(['', '', 'G', '']);
    expect(kept.fixes).toEqual({ bars: [6], chords: [4, 5, 6, 7] });
  });
});
