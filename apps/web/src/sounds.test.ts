import { describe, expect, it } from 'vitest';
import { parseSound, SOUNDS } from './sounds';

describe('sounds', () => {
  it('offers each General MIDI program once', () => {
    const programs = SOUNDS.map((s) => s.program);
    expect(new Set(programs).size).toBe(programs.length);
    expect(programs.every((p) => Number.isInteger(p) && p >= 0 && p < 128)).toBe(true);
    expect(SOUNDS[0]).toEqual({ program: 0, name: 'Grand piano' });
  });

  it('falls back to the piano for a sound no longer offered', () => {
    expect(parseSound(89)).toBe(89);
    expect(parseSound(127)).toBe(0);
    expect(parseSound('89')).toBe(0);
    expect(parseSound(undefined)).toBe(0);
  });
});
