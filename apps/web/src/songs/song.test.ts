import { describe, expect, it } from 'vitest';
import { BUILTIN_SONGS } from './builtin';
import { chartCommands, formatBar, formatChart, marksByBar, parseChart, resolvedBeats } from './song';

describe('chart notation', () => {
  it('spreads one, two or four chords over a bar', () => {
    const { bars, unknown } = parseChart('C | Am F | C . G7 . | Dm Em F G');
    expect(unknown).toEqual([]);
    expect(bars.map((b) => b.beats)).toEqual([
      ['C', '', '', ''],
      ['Am', '', 'F', ''],
      ['C', '', 'G7', ''],
      ['Dm', 'Em', 'F', 'G'],
    ]);
  });

  it('reads sections, fills, repeats and no-chord bars', () => {
    const { bars } = parseChart('[Intro] G7 | [Verse] C | % | [Fill] F | [Chorus] N.C. | [Outro] C');
    expect(bars.map((b) => b.section)).toEqual(['intro', 'A', undefined, undefined, 'B', 'ending']);
    expect(bars[2]!.beats).toEqual(['C', '', '', '']);
    expect(bars[3]!.fill).toBe(true);
    expect(bars[4]!.beats[0]).toBe('N.C.');
  });

  it('reports tokens that are not chords', () => {
    expect(parseChart('C | Hx | [Solo] G').unknown).toEqual(['Hx', '[Solo]']);
  });

  it('formats bars back to notation', () => {
    for (const text of ['C', 'Am F', '. F', 'C D E .', 'C . . G']) {
      expect(formatBar(parseChart(text).bars[0]!)).toBe(text);
    }
    const chart = '[Intro] G7\n[A] C | C | [Fill] F | G';
    expect(formatChart(parseChart(chart).bars)).toBe(chart);
  });

  it('resolves held beats', () => {
    const { bars } = parseChart('C | . F | N.C.');
    expect(resolvedBeats(bars)).toEqual(['C', 'C', 'C', 'C', 'C', 'C', 'F', 'F', null, null, null, null]);
  });
});

describe('engine marks', () => {
  it('marks every intro bar, section starts, fills and the last ending bar', () => {
    const { bars } = parseChart('[Intro] C | C | [A] C | [Fill] C | [B] F | [Ending] C | G | C');
    expect(marksByBar(bars)).toEqual(['intro', 'intro', 'A', 'fill', 'B', null, null, 'ending']);
  });

  it('loads only the beats that change', () => {
    const cmds = chartCommands({ bars: parseChart('[A] C | . Am | N.C.').bars });
    expect(cmds).toEqual([
      { type: 'songClear', bars: 3 },
      { type: 'songChord', beat: 0, slot: { root: 0, quality: 0, bass: 0 } },
      { type: 'songChord', beat: 6, slot: { root: 9, quality: 1, bass: 9 } },
      { type: 'songChord', beat: 8, slot: 'nc' },
      { type: 'songMark', bar: 0, mark: 'A' },
    ]);
  });
});

describe('built-in songs', () => {
  it('parse cleanly and stay in range', () => {
    const ids = new Set<string>();
    for (const song of BUILTIN_SONGS) {
      expect(ids.has(song.id), song.id).toBe(false);
      ids.add(song.id);
      expect(song.bars.length, song.title).toBeGreaterThan(4);
      expect(song.bars[0]!.beats[0], song.title).not.toBe('');
      const chords = chartCommands(song).filter((c) => c.type === 'songChord');
      const tokens = song.bars.flatMap((b) => b.beats).filter(Boolean);
      expect(chords.length, song.title).toBe(tokens.length);
    }
  });
});
