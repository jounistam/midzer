import { describe, expect, it } from 'vitest';
import { BUILTIN_SONGS } from './builtin';
import { barAt, barStarts, chartCommands, formatBar, formatChart, marksByBar, parseChart, resolvedBeats, totalBeats } from './song';

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

  it('reads bars of other lengths from [n/4] tags, until the next one', () => {
    const { bars, unknown } = parseChart('C | [2/4] F | [3/4] G . D | Am F | % | [4/4] C | [5/4] E');
    expect(unknown).toEqual([]);
    expect(bars.map((b) => b.beats)).toEqual([
      ['C', '', '', ''],
      ['F', ''],
      ['G', '', 'D'],
      ['Am', 'F', ''],
      ['Am', 'F', ''],
      ['C', '', '', ''],
      ['E', '', '', '', ''],
    ]);
    // Two chords split an even bar in halves.
    expect(parseChart('[6/4] C G').bars[0]!.beats).toEqual(['C', '', '', 'G', '', '']);
  });

  it('writes the tags back where the length changes, and reads its own notation', () => {
    const text = 'C | [2/4] F | [3/4] G . D | Am F . | [4/4] C | [6/4] C G';
    const { bars } = parseChart(text);
    expect(formatChart(bars)).toBe('C | [2/4] F | [3/4] G . D | Am F .\n[4/4] C | [6/4] C G');
    expect(parseChart(formatChart(bars)).bars).toEqual(bars);
  });

  it('finds the bar of a song beat', () => {
    const { bars } = parseChart('C | [2/4] F | [3/4] G');
    const starts = barStarts(bars);
    expect(starts).toEqual([0, 4, 6, 9]);
    expect(totalBeats(bars)).toBe(9);
    expect([0, 3, 4, 5, 6, 8].map((b) => barAt(starts, b))).toEqual([
      { bar: 0, beat: 0 }, { bar: 0, beat: 3 }, { bar: 1, beat: 0 }, { bar: 1, beat: 1 }, { bar: 2, beat: 0 }, { bar: 2, beat: 2 },
    ]);
    // Past the end, bars as long as the last; with no chart, fours.
    expect(barAt(starts, 13)).toEqual({ bar: 4, beat: 1 });
    expect(barAt([0], 9)).toEqual({ bar: 2, beat: 1 });
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

  it('tells the engine the length of bars other than four and counts their beats on', () => {
    const cmds = chartCommands({ bars: parseChart('C | [2/4] F | [4/4] G').bars });
    expect(cmds).toEqual([
      { type: 'songClear', bars: 3 },
      { type: 'songChord', beat: 0, slot: { root: 0, quality: 0, bass: 0 } },
      { type: 'songBar', bar: 1, beats: 2 },
      { type: 'songChord', beat: 4, slot: { root: 5, quality: 0, bass: 5 } },
      { type: 'songChord', beat: 6, slot: { root: 7, quality: 0, bass: 7 } },
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
