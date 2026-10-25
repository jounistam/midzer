import { describe, expect, it } from 'vitest';
import { CHORD_TONE_HINT, feelWord, homeKey, noteFit, parseAlongMode, parseSoloLevel, SCALE_HINT, soloHints, TAP_HINT } from './solo';

describe('solo levels', () => {
  it('lights the home note from middle C up at level 1', () => {
    expect([...soloHints(1, { tonic: 9, minor: true })]).toEqual([[69, TAP_HINT]]);
    expect(homeKey({ tonic: 0, minor: false })).toBe(60);
    expect(soloHints(1, null).size).toBe(0);
  });

  it('lights the home note, the fifth and the octave at level 2', () => {
    expect([...soloHints(2, { tonic: 7, minor: false })]).toEqual([
      [67, TAP_HINT],
      [74, TAP_HINT],
      [79, TAP_HINT],
    ]);
  });

  it('lights the scale, chord tones brighter, at levels 3 and 4, and nothing at 5', () => {
    const cMajor = 0b0000_1001_0001;
    const hints = soloHints(3, { tonic: 0, minor: false }, cMajor);
    expect([...hints.keys()]).toEqual([60, 62, 64, 67, 69, 72, 74, 76, 79, 81, 84]);
    expect([60, 62, 64, 67].map((k) => hints.get(k))).toEqual([CHORD_TONE_HINT, SCALE_HINT, CHORD_TONE_HINT, CHORD_TONE_HINT]);
    // A minor pentatonic: A C D E G.
    expect([...soloHints(4, { tonic: 9, minor: true }).keys()].slice(0, 5)).toEqual([69, 72, 74, 76, 79]);
    expect(soloHints(5, { tonic: 0, minor: false }, cMajor).size).toBe(0);
  });

  it('tells how a note fits', () => {
    const scale = 0b0010_1001_0101;
    const chord = 0b0000_1001_0001;
    expect([60, 62, 61, 76].map((k) => noteFit(k, scale, chord))).toEqual(['chord', 'scale', 'outside', 'chord']);
  });

  it('reads saved choices', () => {
    expect(parseSoloLevel(1)).toBe(1);
    expect(parseSoloLevel(5)).toBe(5);
    expect(parseSoloLevel(9)).toBe(1);
    expect(parseAlongMode('solo')).toBe('solo');
    expect(parseAlongMode('x')).toBe('chords');
  });

  it('words the feel from calm to fire', () => {
    expect([0, 0.25, 0.5, 0.75, 1].map(feelWord)).toEqual(['Resting', 'Calm', 'Grooving', 'Burning', 'On fire']);
  });
});
