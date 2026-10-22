import { describe, expect, it } from 'vitest';
import { offsetToBeat, SYNC_TAPS, syncedOffset } from './sync';

describe('syncing live audio', () => {
  it('reads a tap against the beat from its sixteenth', () => {
    // At 120 BPM a sixteenth is 125 ms.
    expect(offsetToBeat(-12, 0, 120)).toBe(-12);
    expect(offsetToBeat(10, 1, 120)).toBe(135);
    expect(offsetToBeat(-10, 7, 120)).toBe(-135);
    expect(offsetToBeat(3, 2, 120)).toBeNull();
  });

  it('moves the offset by the median tap', () => {
    const taps = [32, 30, 35, 28, 31, 90, 33, 29, 30, 34, 31, 30];
    expect(syncedOffset(10, taps)).toBe(41);
    expect(syncedOffset(10, taps.slice(0, SYNC_TAPS - 1))).toBeNull();
  });

  it("doesn't trust scattered taps and stays within a second", () => {
    expect(syncedOffset(0, [-80, 60, -40, 90, 0, -100, 70, 30, -60, 100, -20, 50])).toBeNull();
    expect(syncedOffset(950, Array(SYNC_TAPS).fill(120))).toBe(1000);
  });
});
