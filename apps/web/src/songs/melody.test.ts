import { describe, expect, it } from 'vitest';
import { BUILTIN_SONGS } from './builtin';
import { melodyRange, parseMelody } from './melody';
import { BEATS_PER_BAR } from './song';

describe('melody notation', () => {
  it('reads notes, octaves that carry over, lengths and rests', () => {
    const { notes, problems } = parseMelody('C4 D:0.5 E:1/2 r:2 | G3:2 A# Bb5');
    expect(problems).toEqual([]);
    expect(notes).toEqual([
      { at: 0, len: 1, key: 60 },
      { at: 1, len: 0.5, key: 62 },
      { at: 1.5, len: 0.5, key: 64 },
      { at: 4, len: 2, key: 55 },
      { at: 6, len: 1, key: 58 },
      { at: 7, len: 1, key: 82 },
    ]);
    expect(melodyRange(notes)).toEqual({ low: 55, high: 82 });
  });

  it('reports bars that do not add up and tokens that are not notes', () => {
    expect(parseMelody('C D E | F G H').problems).toEqual(['Bar 1 has 3 beats', 'Not a note: H', 'Bar 2 has 2 beats']);
  });

  it('built-in melodies fit their charts', () => {
    const withTunes = BUILTIN_SONGS.filter((s) => s.melody);
    expect(withTunes.length).toBeGreaterThanOrEqual(3);
    for (const song of withTunes) {
      const end = Math.max(...song.melody!.map((n) => n.at + n.len));
      expect(end, song.title).toBeLessThanOrEqual(song.bars.length * BEATS_PER_BAR);
      expect(song.melody![0]!.at, song.title).toBe(BEATS_PER_BAR); // after the intro bar
    }
  });
});
