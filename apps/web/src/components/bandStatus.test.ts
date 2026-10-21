import type { BandState } from '@midzer/audio-web';
import { describe, expect, it } from 'vitest';
import { padState, sectionLine } from './bandStatus';

const band = (state: Partial<BandState>): BandState => ({
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
  energy: 0.4,
  ...state,
});

describe('padState', () => {
  it('lights the playing variation', () => {
    expect(padState('A', band({}))).toBe('on');
    expect(padState('B', band({}))).toBe('off');
  });

  it('blinks the next variation during the fill that leads to it', () => {
    const s = band({ section: 'fill', variation: 0, nextVariation: 2 });
    expect(padState('A', s)).toBe('on');
    expect(padState('C', s)).toBe('queued');
    expect(padState('fill', s)).toBe('on');
  });

  it('shows queued fills and endings', () => {
    expect(padState('fill', band({ fillPending: true }))).toBe('queued');
    expect(padState('ending', band({ endingPending: true }))).toBe('queued');
    expect(padState('ending', band({ section: 'ending' }))).toBe('on');
  });

  it('shows the chosen variation while stopped', () => {
    const s = band({ playing: false, nextVariation: 1 });
    expect(padState('B', s)).toBe('on');
    expect(padState('fill', s)).toBe('off');
  });
});

describe('sectionLine', () => {
  it('describes what plays next', () => {
    expect(sectionLine(band({ style: 1, variation: 1 }))).toBe('Rock · B');
    expect(sectionLine(band({ section: 'fill', nextVariation: 2 }))).toBe('Pop · Fill → C');
    expect(sectionLine(band({ fillPending: true }))).toBe('Pop · A → Fill');
    expect(sectionLine(band({ section: 'rest' }))).toBe('Pop · Waiting for you');
  });
});
