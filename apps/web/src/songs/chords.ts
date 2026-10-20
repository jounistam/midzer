// Chord names as people write them in charts and ChordPro files, mapped onto
// the engine's chord qualities (see `Quality::ALL` in chord.rs).

import { chordName, type ChordSpec } from '@midzer/audio-web';

/** Quality indices in engine order. */
export const Q = {
  major: 0,
  minor: 1,
  dim: 2,
  aug: 3,
  sus2: 4,
  sus4: 5,
  power: 6,
  major6: 7,
  minor6: 8,
  dom7: 9,
  maj7: 10,
  min7: 11,
  halfDim7: 12,
  dim7: 13,
  add9: 14,
} as const;

const LETTERS: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };

/**
 * Suffix patterns, tried in order. Extensions the engine has no quality for
 * fall back to the nearest one it has (C9 → C7, Cm11 → Cm7, C7sus4 → Csus4).
 */
const SUFFIXES: [RegExp, number][] = [
  [/^$|^(maj|M)$/, Q.major],
  [/^(m|min|-)$/, Q.minor],
  [/^(dim|°|o)$/, Q.dim],
  [/^(aug|\+|\+5|#5)$/, Q.aug],
  [/^(sus2|2)$/, Q.sus2],
  [/^(sus4?|\d+sus4?)$/, Q.sus4],
  [/^5$/, Q.power],
  [/^(6|maj6|M6|6\/9|69)$/, Q.major6],
  [/^(m6|min6|-6|m6\/9|m69)$/, Q.minor6],
  [/^(m7b5|m7-5|min7b5|-7b5|ø7?)$/, Q.halfDim7],
  [/^(dim7|°7|o7)$/, Q.dim7],
  [/^(maj|M|Δ|ma)(7|9|11|13)?([#b+-]\d+)*$|^Δ$/, Q.maj7],
  [/^(m|min|-)(7|9|11|13)([#b+-]\d+)*$/, Q.min7],
  [/^(m|min|-)(maj7|M7)$/, Q.minor],
  [/^(7|9|11|13)([#b+-]\d+|alt)*$/, Q.dom7],
  [/^(add9|add2)$/, Q.add9],
  [/^(m|min|-)(add9|add2)$/, Q.minor],
];

function pitchClass(text: string): [pc: number, rest: string] | null {
  const letter = LETTERS[text[0] ?? ''];
  if (letter === undefined) return null;
  let pc = letter;
  let i = 1;
  while (i < text.length && '#♯b♭'.includes(text[i]!)) {
    pc += text[i] === '#' || text[i] === '♯' ? 1 : -1;
    i++;
  }
  return [(pc + 12) % 12, text.slice(i)];
}

/** Parses "Am7", "F#m7b5", "Bb/D", "Gsus" and the like; null if it is not a chord. */
export function parseChord(name: string): ChordSpec | null {
  const text = name.trim().replace(/[()]/g, '');
  const root = pitchClass(text);
  if (!root) return null;
  const rootPc = root[0];
  let rest = root[1];
  let bass = rootPc;
  const slash = rest.lastIndexOf('/');
  // "6/9" is a quality, not a slash chord.
  if (slash >= 0 && !/^\d/.test(rest.slice(slash + 1))) {
    const b = pitchClass(rest.slice(slash + 1));
    if (!b || b[1] !== '') return null;
    bass = b[0];
    rest = rest.slice(0, slash);
  }
  const match = SUFFIXES.find(([re]) => re.test(rest));
  if (!match) return null;
  return { root: rootPc, quality: match[1], bass };
}

/** The engine's name for a chord, e.g. "Am7/C". */
export function formatChord(chord: ChordSpec): string {
  return chordName(chord.root, chord.quality, chord.bass);
}

/** "N.C." and friends: no chord, only drums. */
export function isNoChord(token: string): boolean {
  return /^(n\.?c\.?|x)$/i.test(token.trim());
}
