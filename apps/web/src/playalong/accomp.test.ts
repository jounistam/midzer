import { describe, expect, it } from 'vitest';
import { accompHints, ACCOMP_PATTERNS, parseAccompLevel, parseAccompPattern, patternIndex } from './accomp';
import { TAP_HINT } from './solo';

describe('accompany', () => {
  it('lights the chord’s root to tap from middle C, or below a split', () => {
    const am = { root: 9, quality: 1 };
    expect([...accompHints(1, am)]).toEqual([[69, TAP_HINT]]);
    expect([...accompHints(2, am, 60)]).toEqual([[57, TAP_HINT]]);
    expect(accompHints(1, null).size).toBe(0);
  });

  it('keeps the engine’s pattern order and reads old prefs', () => {
    expect(ACCOMP_PATTERNS.map((p) => p.id)).toEqual(['arpeggio', 'broken', 'stabs', 'ballad', 'bounce']);
    expect(patternIndex('bounce')).toBe(4);
    expect(parseAccompPattern('stabs')).toBe('stabs');
    expect(parseAccompPattern('waltz')).toBe('arpeggio');
    expect(parseAccompLevel(2)).toBe(2);
    expect(parseAccompLevel(7)).toBe(1);
  });
});
