import { describe, expect, it } from 'vitest';
import { tapOffset } from './calibrate';

const beats = Array.from({ length: 12 }, (_, i) => 1000 + i * 500);

describe('tapOffset', () => {
  it('finds the median lateness', () => {
    const taps = [40, 35, 50, 45, 38, 42, 300, 41].map((o, i) => beats[i + 2]! + o);
    expect(tapOffset(taps, beats)).toBe(41);
  });

  it('handles early taps', () => {
    const taps = [-20, -25, -18, -22, -21, -19].map((o, i) => beats[i + 1]! + o);
    expect(tapOffset(taps, beats)).toBe(-20.5);
  });

  it('rejects random tapping', () => {
    const taps = [10, 160, -140, 90, -60, 150, 20, -150].map((o, i) => beats[i + 2]! + o);
    expect(tapOffset(taps, beats)).toBeNull();
  });
});
