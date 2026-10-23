import { describe, expect, it } from 'vitest';
import type { LiveChordSpan } from '@midzer/audio-web';
import { ChordTimeline } from './chords';

const span = (fromMs: number, toMs: number, name: string | null, predicted = false): LiveChordSpan => ({
  fromMs,
  toMs,
  chord: name === null ? null : { root: 0, bass: 0, quality: 0, name },
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
});
