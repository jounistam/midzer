import { describe, expect, it } from 'vitest';
import { boundaryX, keyAt, layoutKeys } from './keyboardLayout';

describe('layoutKeys', () => {
  const keys = layoutKeys(48, 72, 1500, 200); // C3..C5: 15 white keys

  it('fills the width with white keys', () => {
    const whites = keys.filter((k) => !k.black);
    expect(whites).toHaveLength(15);
    expect(whites[0]!.x).toBe(0);
    expect(whites.at(-1)!.x + whites.at(-1)!.w).toBeCloseTo(1500);
  });

  it('places black keys between their neighbours', () => {
    expect(keys.filter((k) => k.black)).toHaveLength(10);
    const cSharp = keys.find((k) => k.note === 49)!;
    expect(cSharp.x + cSharp.w / 2).toBeCloseTo(100); // boundary between C and D
  });

  it('hit-tests black keys over white keys', () => {
    expect(keyAt(keys, 100, 50)).toBe(49); // upper half: C#
    expect(keyAt(keys, 100, 180)).toBe(50); // lower half: D
    expect(keyAt(keys, 10, 180)).toBe(48);
    expect(keyAt(keys, 2000, 10)).toBeNull();
  });

  it('finds the split boundary', () => {
    expect(boundaryX(keys, 60)).toBeCloseTo(700);
    expect(boundaryX(keys, 61)).toBeNull();
  });
});
