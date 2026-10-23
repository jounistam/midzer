import { describe, expect, it } from 'vitest';
import { keyNames, parseLevel, voicing } from './levels';

// Qualities by index in QUALITY_SUFFIXES: 0 major, 1 m, 9 7, 11 m7.
const C = { root: 0, quality: 0 };
const G = { root: 7, quality: 0 };
const Am = { root: 9, quality: 1 };
const G7 = { root: 7, quality: 9 };

describe('play-along levels', () => {
  it('starts with one key for the right hand', () => {
    expect(voicing(Am, 1)).toEqual({ left: [], right: [69] });
  });

  it('plays the triad in root position at level 2, leaving out the seventh', () => {
    expect(voicing(Am, 2).right).toEqual([69, 72, 76]);
    expect(keyNames(voicing(G7, 2).right)).toBe('G B D');
  });

  it('turns chords around near the middle of the keyboard at level 3', () => {
    expect(voicing(C, 3).right).toEqual([60, 64, 67]);
    expect(voicing(G, 3).right).toEqual([62, 67, 71]);
    const all = [C, G, Am, G7].flatMap((c) => voicing(c, 3).right);
    expect(Math.max(...all) - Math.min(...all)).toBeLessThanOrEqual(12);
  });

  it('adds the left hand at level 4 and the full chord at level 5', () => {
    expect(voicing(G, 4)).toEqual({ left: [43], right: [62, 67, 71] });
    const full = voicing(G7, 5);
    expect(full.left).toEqual([43, 50]);
    expect(keyNames(full.right).split(' ').sort()).toEqual(['B', 'D', 'F', 'G']);
  });

  it('reads a saved level, falling back to the first', () => {
    expect(parseLevel(4)).toBe(4);
    expect(parseLevel(9)).toBe(1);
    expect(parseLevel('2')).toBe(1);
  });
});
