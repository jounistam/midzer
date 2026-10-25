import { describe, expect, it } from 'vitest';
import { CHORD_TONE_HINT, drumSoloHints, feelWord, parseDrumSoloLevel, homeKey, NO_CALL, nextCall, noteFit, parseAlongMode, parseSoloLevel, SCALE_HINT, soloHints, TAP_HINT } from './solo';

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

  it('lights the call’s note to play from level 3', () => {
    const c = { tonic: 0, minor: false };
    expect(soloHints(3, c, 0, 67).get(67)).toBe(TAP_HINT);
    expect(soloHints(5, c, 0, 50)).toEqual(new Map([[50, TAP_HINT]]));
    expect(soloHints(1, c, 0, 67).has(67)).toBe(false);
  });

  it('follows call and response, keeping the last score', () => {
    const call = (phase: 'idle' | 'listen' | 'answer' | 'scored', hit: number, of: number, key: number | null = null) => ({ type: 'soloCall', phase, hit, of, key, frame: 0 }) as const;
    let now = nextCall(NO_CALL, call('listen', 0, 3));
    now = nextCall(now, call('listen', 1, 3, 64));
    expect(now).toEqual({ phase: 'listen', hit: 1, of: 3, key: 64, last: null });
    now = nextCall(now, call('answer', 0, 3, 60));
    now = nextCall(now, call('scored', 2, 3));
    expect(now.last).toEqual({ hit: 2, of: 3 });
    expect(now.phase).toBe('answer');
    now = nextCall(now, call('listen', 0, 3));
    expect(now).toEqual({ phase: 'listen', hit: 0, of: 3, key: null, last: { hit: 2, of: 3 } });
    expect(nextCall(now, call('idle', 0, 0))).toEqual(NO_CALL);
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

describe('drum solo levels', () => {
  it('lights the pads to tap, then the player’s part, then nothing', () => {
    expect([...drumSoloHints(1)]).toEqual([['snare', TAP_HINT]]);
    expect([...drumSoloHints(2).keys()]).toEqual(['kick', 'snare', 'tom1', 'tom2', 'tom3', 'tom4']);
    expect([...drumSoloHints(3)]).toEqual([
      ['kick', SCALE_HINT],
      ['snare', SCALE_HINT],
    ]);
    expect(drumSoloHints(4).size).toBe(0);
  });

  it('lights the call’s next hit from level 3', () => {
    expect(drumSoloHints(3, 43).get('tom3')).toBe(TAP_HINT);
    expect(drumSoloHints(3, 36).get('kick')).toBe(TAP_HINT);
    expect(drumSoloHints(1, 43).has('tom3')).toBe(false);
  });

  it('parses saved levels', () => {
    expect(parseDrumSoloLevel(3)).toBe(3);
    expect(parseDrumSoloLevel(5)).toBe(1);
  });
});
