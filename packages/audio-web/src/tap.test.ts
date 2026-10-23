import { describe, expect, it } from 'vitest';
import { TapRing } from './tap';

describe('TapRing', () => {
  it('passes blocks with their engine frames, past 32 bits', () => {
    const ring = TapRing.create(4);
    const block = new Float32Array(128).map((_, i) => i / 128);
    expect(ring.push(block, 128, 2 ** 32 + 5)).toBe(true);
    expect(ring.push(block, 64, 2 ** 32 + 133)).toBe(true);
    const out = new Float32Array(128);
    expect(ring.pop(out)).toBe(128);
    expect(ring.frame).toBe(2 ** 32 + 5);
    expect(out[127]).toBeCloseTo(127 / 128);
    expect(ring.pop(out)).toBe(64);
    expect(ring.frame).toBe(2 ** 32 + 133);
    expect(ring.pop(out)).toBe(0);
  });

  it('refuses blocks when full and carries the flags', () => {
    const ring = TapRing.create(2);
    const block = new Float32Array(128);
    expect(ring.push(block, 128, 0)).toBe(true);
    expect(ring.push(block, 128, 128)).toBe(true);
    expect(ring.push(block, 128, 256)).toBe(false);
    ring.clear();
    expect(ring.pop(new Float32Array(128))).toBe(0);
    const other = new TapRing(ring.buffer);
    other.enabled = true;
    other.frameOffset = -1234;
    expect(ring.enabled).toBe(true);
    expect(ring.frameOffset).toBe(-1234);
  });
});
