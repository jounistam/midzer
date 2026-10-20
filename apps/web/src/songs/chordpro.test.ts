import { describe, expect, it } from 'vitest';
import { parseChordPro } from './chordpro';

describe('parseChordPro', () => {
  it('reads metadata, sections, chords and lyrics', () => {
    const { song, unknown } = parseChordPro(`
{title: Test Song}
{artist: Someone}
{tempo: 92}
# a comment
{start_of_verse}
[C]Hello, [G]world, it's a [Am]song
we sing a[F]long
{end_of_verse}
{soc}
[F]La la [C]la [Zz]la
{eoc}
{c: Bridge}
| Dm . G . | C |
`);
    expect(song.title).toBe('Test Song');
    expect(song.artist).toBe('Someone');
    expect(song.bpm).toBe(92);
    expect(unknown).toEqual(['Zz']);
    expect(song.bars.map((b) => b.beats.filter(Boolean).join(' '))).toEqual(['C', 'G', 'Am', 'F', 'F', 'C', 'Dm G', 'C']);
    expect(song.bars.map((b) => b.section)).toEqual(['A', undefined, undefined, undefined, 'B', undefined, 'C', undefined]);
    expect(song.bars.map((b) => b.lyric)).toEqual([
      'Hello,',
      "world, it's a",
      'song we sing a',
      'long',
      'La la',
      'la la',
      undefined,
      undefined,
    ]);
  });

  it('reads grids and plain section labels', () => {
    const { song } = parseChordPro('Chorus:\n{start_of_grid}\n| G . . . | D . . . |\n{end_of_grid}\n[Em]Words');
    expect(song.bars.map((b) => b.beats[0])).toEqual(['G', 'D', 'Em']);
    expect(song.bars[0]!.section).toBe('B');
  });

  it('falls back to a title', () => {
    expect(parseChordPro('[C]x', 'file name').song.title).toBe('file name');
  });
});
