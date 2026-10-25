import { describe, expect, it } from 'vitest';
import { feelWord, homeKey, parseAlongMode, parseSoloLevel, soloHints, TAP_HINT } from './solo';

describe('solo levels', () => {
  it('lights the home note from middle C up at level 1', () => {
    expect([...soloHints(1, { tonic: 9, minor: true })]).toEqual([[69, TAP_HINT]]);
    expect(homeKey({ tonic: 0, minor: false })).toBe(60);
    expect(soloHints(1, null).size).toBe(0);
  });

  it('reads saved choices', () => {
    expect(parseSoloLevel(1)).toBe(1);
    expect(parseSoloLevel(9)).toBe(1);
    expect(parseAlongMode('solo')).toBe('solo');
    expect(parseAlongMode('x')).toBe('chords');
  });

  it('words the feel from calm to fire', () => {
    expect([0, 0.25, 0.5, 0.75, 1].map(feelWord)).toEqual(['Resting', 'Calm', 'Grooving', 'Burning', 'On fire']);
  });
});
