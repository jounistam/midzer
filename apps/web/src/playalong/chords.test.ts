import { describe, expect, it } from 'vitest';
import type { LiveChordSpan } from '@midzer/audio-web';
import { chordBlocks, ChordTimeline, keyHints, snapToBeats } from './chords';

const ROOTS: Record<string, number> = { C: 0, F: 5, G: 7 };
const span = (fromMs: number, toMs: number, name: string | null, predicted = false): LiveChordSpan => ({
  fromMs,
  toMs,
  chord: name === null ? null : { root: ROOTS[name] ?? 0, bass: 0, quality: 0, name },
  predicted,
});

describe('ChordTimeline', () => {
  it('shows the chord heard now and the ones before it', () => {
    const t = new ChordTimeline();
    t.add([span(0, 1000, 'C'), span(1000, 2000, 'Am'), span(2000, 3000, null), span(3000, 4000, 'F')], 0);
    expect(t.at(1500)).toEqual({ current: 'Am', before: ['C'], next: null });
    expect(t.at(3500)).toEqual({ current: 'F', before: ['Am', 'C'], next: null });
  });

  it('carries the newest chord on until the next batch, but not for ever', () => {
    const t = new ChordTimeline();
    t.add([span(0, 1000, 'G')], 0);
    expect(t.at(3000).current).toBe('G');
    expect(t.at(9000).current).toBeNull();
  });

  it('lets a later batch correct what came before, in the player time', () => {
    const t = new ChordTimeline();
    t.add([span(0, 1000, 'C'), span(1000, 2000, 'E')], 0);
    t.add([span(500, 1200, 'C'), span(1200, 2500, 'Em')], 100);
    expect(t.at(1250).current).toBe('C');
    expect(t.at(1400)).toEqual({ current: 'Em', before: ['C'], next: null });
  });

  it('shows what the loop says comes next, and then plays it on time', () => {
    const t = new ChordTimeline();
    t.add([span(0, 1000, 'C'), span(1000, 2000, 'G'), span(2000, 2500, 'G', true), span(2500, 3500, 'Am', true)], 0);
    expect(t.at(1800)).toEqual({ current: 'G', before: ['C'], next: { name: 'Am', inMs: 700 } });
    expect(t.at(2200).current).toBe('G');
    expect(t.at(2600)).toEqual({ current: 'Am', before: ['G', 'C'], next: null });
  });

  it('carries the newest chord on to now in the moving view', () => {
    const t = new ChordTimeline();
    t.add([span(0, 1000, 'C'), span(1000, 2000, 'G')], 0);
    expect(t.window(500, 5000, 3000).map((s) => [s.name, s.fromMs, s.toMs])).toEqual([
      ['C', 0, 1000],
      ['G', 1000, 3000],
    ]);
    expect(t.window(500, 5000, 9000).at(-1)!.toMs).toBe(2000);
  });
});

describe('keyHints', () => {
  const t = new ChordTimeline();
  t.add([span(0, 1000, 'C'), span(1000, 2000, 'C', true), span(2000, 3000, 'F', true)], 0);
  const spans = t.window(0, 4000, 0);

  it('lights the chord heard now, by hand', () => {
    expect([...keyHints(spans, 500, 4, 600)]).toEqual([
      [36, 1],
      [60, 2],
      [64, 2],
      [67, 2],
    ]);
  });

  it('shows the next chord coming, then switches to it just before it starts', () => {
    expect(keyHints(spans, 1300, 1, 600).size).toBe(1);
    expect([...keyHints(spans, 1600, 1, 600)]).toEqual([
      [65, 3],
      [60, 2],
    ]);
    expect([...keyHints(spans, 1900, 1, 600)]).toEqual([[65, 2]]);
  });
});

describe('chordBlocks', () => {
  it('joins a chord heard with the same chord predicted after it, and leaves out no chord', () => {
    const t = new ChordTimeline();
    t.add([span(0, 1000, 'C'), span(1000, 1500, null), span(1500, 2000, 'G'), span(2000, 3000, 'G', true), span(3000, 4000, 'C', true)], 0);
    expect(chordBlocks(t.window(0, 5000, 2000)).map((b) => [b.name, b.fromMs, b.toMs, b.predicted])).toEqual([
      ['C', 0, 1000, false],
      ['G', 1500, 3000, false],
      ['C', 3000, 4000, true],
    ]);
  });
});

describe('snapToBeats', () => {
  const heard = (fromMs: number, toMs: number, name: string) => ({ fromMs, toMs, name, chord: { root: ROOTS[name] ?? 0, quality: 0 }, predicted: false });

  it('moves chord changes near a beat onto it, and leaves the rest', () => {
    const out = snapToBeats([heard(-30, 1040, 'C'), heard(1040, 1250, 'G'), heard(1250, 2010, 'F')], 0, 500);
    expect(out.map((s) => [s.name, s.fromMs, s.toMs])).toEqual([
      ['C', 0, 1000],
      ['G', 1000, 1250],
      ['F', 1250, 2000],
    ]);
  });

  it('drops a sliver left between two beats, and does nothing without a beat', () => {
    expect(snapToBeats([heard(980, 1030, 'G')], 0, 500)).toEqual([]);
    expect(snapToBeats([heard(980, 1030, 'G')], -Infinity, 500)).toHaveLength(1);
  });
});
