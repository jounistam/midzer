import type { BandState } from '@midzer/audio-web';
import { describe, expect, it } from 'vitest';
import { countInPieces, nextChord, stageSection, TimingMeter, timingVerdict } from './stage';

const band: BandState = {
  style: 0,
  nextStyle: null,
  playing: true,
  section: 'main',
  variation: 0,
  nextVariation: 0,
  fillPending: false,
  introPending: false,
  endingPending: false,
  follow: true,
  cued: false,
  energy: 0.5,
};

describe('nextChord', () => {
  const beats = ['C', 'C', 'C', 'C', 'G', 'G', 'Am', 'Am'];
  it('finds the next change and wraps around loops', () => {
    expect(nextChord(beats, 1)).toEqual({ name: 'G', in: 3 });
    expect(nextChord(beats, 6)).toBeNull();
    expect(nextChord(beats, 6, true)).toEqual({ name: 'C', in: 2 });
    expect(nextChord(beats, 9, true)).toEqual({ name: 'G', in: 3 });
    expect(nextChord(['C', null], 0)).toEqual({ name: 'N.C.', in: 1 });
    expect(nextChord([], 0)).toBeNull();
  });
});

describe('stageSection', () => {
  it('tells the drummer what a fill and a crash will do', () => {
    expect(stageSection(band)).toMatchObject({ now: 'A', next: null });
    expect(stageSection({ ...band, cued: true, nextVariation: 1 })).toEqual({ now: 'A', next: 'B', hint: 'Crash on the 1 for B' });
    expect(stageSection({ ...band, section: 'fill', nextVariation: 2 })).toMatchObject({ now: 'Fill', next: 'C' });
    expect(stageSection({ ...band, endingPending: true })).toMatchObject({ next: 'End' });
    expect(stageSection({ ...band, section: 'rest' }).hint).toMatch(/bring the band back/);
    expect(stageSection({ ...band, playing: false, nextVariation: 2 })).toMatchObject({ now: 'C', next: null });
  });
});

describe('timing', () => {
  it('averages hits and says rushing, dragging or in the pocket', () => {
    const meter = new TimingMeter();
    for (const ms of [-20, -18, -22, -19, -21]) meter.push(ms);
    expect(meter.average).toBeLessThan(-15);
    expect(timingVerdict(meter.average, meter.hits)).toMatch(/^Rushing 2\d ms$/);
    expect(timingVerdict(12, 10)).toBe('Dragging 12 ms');
    expect(timingVerdict(-3, 10)).toBe('In the pocket');
    expect(timingVerdict(-30, 2)).toBe('Listening…');
    meter.reset();
    expect(meter.hits).toBe(0);
  });

  it('maps count-in choices to pieces', () => {
    expect(countInPieces('hiHat')).toContain('hiHatPedal');
    expect(countInPieces('any')).toHaveLength(17);
    expect(countInPieces('off')).toEqual([]);
  });
});
