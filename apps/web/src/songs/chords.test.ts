import { describe, expect, it } from 'vitest';
import { formatChord, isNoChord, parseChord } from './chords';

const name = (text: string) => {
  const c = parseChord(text);
  return c ? formatChord(c) : null;
};

describe('parseChord', () => {
  it('reads roots, accidentals and slash basses', () => {
    expect(name('C')).toBe('C');
    expect(name('F#m')).toBe('F#m');
    expect(name('Bbmaj7')).toBe('Bbmaj7');
    expect(name('Db')).toBe('C#');
    expect(name('C/E')).toBe('C/E');
    expect(name('Am7/G')).toBe('Am7/G');
  });

  it('understands common spellings', () => {
    expect(name('Cmin')).toBe('Cm');
    expect(name('C-7')).toBe('Cm7');
    expect(name('CM7')).toBe('Cmaj7');
    expect(name('CΔ')).toBe('Cmaj7');
    expect(name('Bø')).toBe('Bm7b5');
    expect(name('Bm7b5')).toBe('Bm7b5');
    expect(name('Co7')).toBe('Cdim7');
    expect(name('C+')).toBe('Caug');
    expect(name('Gsus')).toBe('Gsus4');
    expect(name('C6/9')).toBe('C6');
    expect(name('C(add9)')).toBe('Cadd9');
  });

  it('maps extensions to the nearest quality', () => {
    expect(name('G9')).toBe('G7');
    expect(name('G7b9')).toBe('G7');
    expect(name('G13')).toBe('G7');
    expect(name('Dm11')).toBe('Dm7');
    expect(name('Fmaj9')).toBe('Fmaj7');
    expect(name('G7sus4')).toBe('Gsus4');
    expect(name('Cm(maj7)')).toBe('Cm');
  });

  it('rejects words that are not chords', () => {
    expect(parseChord('Hello')).toBeNull();
    expect(parseChord('am')).toBeNull();
    expect(parseChord('Cxyz')).toBeNull();
    expect(parseChord('C/X')).toBeNull();
  });

  it('knows no-chord marks', () => {
    expect(isNoChord('N.C.')).toBe(true);
    expect(isNoChord('NC')).toBe(true);
    expect(isNoChord('C')).toBe(false);
  });
});
