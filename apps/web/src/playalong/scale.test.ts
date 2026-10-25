import { describe, expect, it } from 'vitest';
import { QUALITY_SUFFIXES } from '@midzer/audio-web';
import { chordBits, KeyFollower, keyName, keyOf, pentatonic, type WeightedChord } from './scale';

const q = (suffix: string) => QUALITY_SUFFIXES.indexOf(suffix as (typeof QUALITY_SUFFIXES)[number]);
const NOTES: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11, Bb: 10, Eb: 3, 'F#': 6 };
/** "Am:4 F:4" → chords with their beats. */
function chords(line: string): WeightedChord[] {
  return line.split(' ').map((item) => {
    const [name, beats] = item.split(':');
    const m = /^([A-G](?:b|#)?)(.*)$/.exec(name!)!;
    return { chord: { root: NOTES[m[1]!]!, quality: q(m[2]!) }, weight: Number(beats ?? 4) };
  });
}

describe('keyOf', () => {
  it('finds major keys from their chords', () => {
    expect(keyName(keyOf(chords('C G Am F'))!)).toBe('C major');
    expect(keyName(keyOf(chords('G D Em C'))!)).toBe('G major');
    expect(keyName(keyOf(chords('D A Bm G'))!)).toBe('D major');
  });

  it('tells a minor key from its relative major by the chords on the home note', () => {
    expect(keyName(keyOf(chords('Am:8 F G Am:8 E7'))!)).toBe('A minor');
    expect(keyName(keyOf(chords('Em:8 C D Em:8 B7'))!)).toBe('E minor');
  });

  it('a blues in A is in A', () => {
    expect(keyOf(chords('A7:16 D7:8 A7:8 E7 D7 A7:8'))!.tonic).toBe(9);
  });

  it('has no key without chords', () => {
    expect(keyOf([])).toBeNull();
  });
});

describe('scales and chords as bits', () => {
  it('gives the pentatonics', () => {
    // C D E G A
    expect(pentatonic({ tonic: 0, minor: false })).toBe(0b0010_1001_0101);
    // A C D E G: the same notes as C major's.
    expect(pentatonic({ tonic: 9, minor: true })).toBe(0b0010_1001_0101);
  });

  it('gives chord tones', () => {
    expect(chordBits({ root: 9, quality: q('m') })).toBe((1 << 9) | (1 << 0) | (1 << 4));
    expect(chordBits(null)).toBe(0);
  });
});

describe('KeyFollower', () => {
  it('keeps its key until another fits clearly better for a while', () => {
    const follower = new KeyFollower();
    expect(follower.update(chords('C G Am F'), 0)).toEqual({ tonic: 0, minor: false });
    // A move to D major: kept at first...
    expect(follower.update(chords('D A Bm G'), 1000)!.tonic).toBe(0);
    expect(follower.update(chords('D A Bm G'), 5000)!.tonic).toBe(0);
    // ... taken once it has held.
    expect(follower.update(chords('D A Bm G'), 9000)!.tonic).toBe(2);
  });

  it('keeps a key set for a followed song', () => {
    const follower = new KeyFollower();
    follower.set({ tonic: 7, minor: false });
    expect(follower.key).toEqual({ tonic: 7, minor: false });
    follower.reset();
    expect(follower.key).toBeNull();
  });
});
