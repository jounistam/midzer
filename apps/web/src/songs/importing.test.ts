import { describe, expect, it } from 'vitest';
import { importText } from './importing';

describe('importText', () => {
  it('reads chart notation', () => {
    const { song, notes } = importText('[Verse] C | Am | F G | Qq', 'Chart');
    expect(song.title).toBe('Chart');
    expect(song.bars.length).toBe(3);
    expect(song.bars[0]!.section).toBe('A');
    expect(notes).toEqual(["Skipped what didn't read as chords: Qq"]);
  });

  it('reads ChordPro when it has inline chords or directives', () => {
    expect(importText('[Am]Hello [F]there').song.bars.map((b) => b.beats[0])).toEqual(['Am', 'F']);
    expect(importText('{title: T}\n[C]x').song.title).toBe('T');
  });

  it('explains when there are no chords', () => {
    expect(() => importText('just some words')).toThrow(/No chords found/);
  });
});
