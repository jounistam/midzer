import { describe, expect, it } from 'vitest';
import { melody } from '../songs/melody';
import { songFromChart } from '../songs/song';
import { buildTargets, partsOf, targetsEnd } from './targets';

const song = {
  ...songFromChart({ id: 't', title: 'T', bpm: 100, style: 0, source: 'user' }, '[Intro] C | [A] C | Am F | G7 | [Ending] C'),
  melody: melody('r:4 | E4 D C:2 | C:2 A3:2 | B3:4 | C4:4'),
};

describe('targets', () => {
  it('asks for chords where they change and on every bar, but not in the intro', () => {
    const chords = buildTargets(song, 'chords');
    expect(chords.map((t) => [t.beat, t.name, t.len])).toEqual([
      [4, 'C', 4],
      [8, 'Am', 2],
      [10, 'F', 2],
      [12, 'G7', 4],
      [16, 'C', 4],
    ]);
    expect(chords[1]!.pcs).toEqual([9, 0, 4]);
    expect(chords[1]!.keys).toEqual([57, 60, 64]);
    // Four-note chords may leave out the fifth.
    expect(chords[3]!.pcs).toEqual([7, 11, 5]);
    expect(targetsEnd(chords)).toBe(20);
  });

  it('melody targets are the notes; hands together puts chords first on shared beats', () => {
    expect(buildTargets(song, 'melody').map((t) => [t.beat, t.keys[0]])).toEqual([
      [4, 64], [5, 62], [6, 60], [8, 60], [10, 57], [12, 59], [16, 60],
    ]);
    const both = buildTargets(song, 'both');
    expect(both.slice(0, 2).map((t) => t.kind)).toEqual(['chord', 'note']);
    expect(both.map((t) => t.id)).toEqual(both.map((_, i) => i));
  });

  it('offers melody parts only for songs with a melody', () => {
    expect(partsOf(song)).toEqual(['melody', 'chords', 'both']);
    expect(partsOf({ ...song, melody: undefined })).toEqual(['chords']);
  });
});
