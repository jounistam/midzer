import { describe, expect, it } from 'vitest';
import { describeOffset, HISTOGRAM_RANGE_MS, HISTOGRAM_STEP_MS, summarize, type JudgedHit } from './judging';

/** Two bars of rock: kick on 1 and 3, snare `snareLate` ms late on 2 and 4, hi-hat eighths `hatMs` off. */
function groove(bars: number, snareLate: number, hatMs = 0): JudgedHit[] {
  const hits: JudgedHit[] = [];
  for (let bar = 0; bar < bars; bar++) {
    for (let s = 0; s < 16; s += 2) {
      hits.push({ ms: hatMs, piece: 'hiHatClosed', sixteenth: s, bar });
      if (s === 0 || s === 8) hits.push({ ms: -2, piece: 'kick', sixteenth: s, bar });
      if (s === 4 || s === 12) hits.push({ ms: snareLate, piece: 'snare', sixteenth: s, bar });
    }
  }
  return hits;
}

describe('judging against a recording', () => {
  it('says where a part leans', () => {
    const summary = summarize(groove(4, 12));
    const snare = summary.parts.find((p) => p.id === 'snare')!;
    expect(snare).toMatchObject({ hits: 8, onTime: 1, lean: '12 ms late on the backbeat' });
    expect(snare.meanMs).toBeCloseTo(12);
    expect(summary.parts.find((p) => p.id === 'kick')!.lean).toBeNull();
    expect(summary.parts.map((p) => p.id)).toEqual(['kick', 'snare', 'hiHat']);
  });

  it('counts hits on time, bar by bar, and bins the offsets', () => {
    const hits = [...groove(2, 0), ...groove(1, 30).map((h) => ({ ...h, bar: 2 }))];
    const summary = summarize(hits);
    expect(summary.hits).toBe(36);
    expect(summary.bars.map((b) => [b.bar, b.onTime])).toEqual([
      [0, 1],
      [1, 1],
      [2, 10 / 12],
    ]);
    expect(summary.onTime).toBeCloseTo(34 / 36);
    expect(summary.histogram).toHaveLength((2 * HISTOGRAM_RANGE_MS) / HISTOGRAM_STEP_MS);
    expect(summary.histogram.reduce((a, b) => a + b)).toBe(36);
    // 30 ms late lands in the 30–35 bin; -2 in the -5–0 bin.
    expect(summary.histogram[(30 + HISTOGRAM_RANGE_MS) / HISTOGRAM_STEP_MS]).toBe(2);
    expect(summary.histogram[(HISTOGRAM_RANGE_MS - 5) / HISTOGRAM_STEP_MS]).toBe(6);
  });

  it('leaves wild hits out of the averages but not the counts', () => {
    const hits: JudgedHit[] = [...groove(2, 0, 4), { ms: 110, piece: 'snare', sixteenth: 6, bar: 1 }];
    const summary = summarize(hits);
    expect(summary.spreadMs).toBeLessThan(4);
    expect(summary.histogram.at(-1)).toBe(1);
    expect(summary.parts.find((p) => p.id === 'snare')!.meanMs).toBeCloseTo(0);
  });

  it('describes offsets in words', () => {
    expect(describeOffset(12.4)).toBe('12 ms late');
    expect(describeOffset(-8)).toBe('8 ms early');
    expect(describeOffset(2)).toBe('on time');
    expect(summarize([]).parts).toEqual([]);
  });
  it('sums up keys strikes and the chords checked against the chart', () => {
    const hits: JudgedHit[] = [
      { ms: 4, piece: 'keys', chord: true, sixteenth: 0, bar: 0 },
      { ms: -20, piece: 'keys', chord: false, sixteenth: 8, bar: 0 },
      { ms: 2, piece: 'keys', chord: null, sixteenth: 10, bar: 0 },
      { ms: 0, piece: 'keys', chord: true, sixteenth: 0, bar: 1 },
      { ms: -2, piece: 'kick', sixteenth: 0, bar: 1 },
    ];
    const summary = summarize(hits);
    expect(summary.chords).toEqual({ checked: 3, right: 2 });
    expect(summary.parts.find((p) => p.id === 'keys')).toMatchObject({ hits: 4, onTime: 0.75 });
    expect(summarize(groove(1, 0)).chords).toBeNull();
  });
});
