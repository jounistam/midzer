// Melodies: the notes a song's tune is made of, for Learn. Written in a small
// notation for built-in songs, or read from a MIDI file's melody track.
//
// Notation: notes separated by spaces and bar lines `|`. A note is a letter,
// an optional sharp or flat and an optional octave (`C4` is middle C; the
// octave carries over to the next notes), then an optional `:length` in beats
// (`:2`, `:0.5`, `:1/2`; one beat if left out). `r` is a rest. Bar lines are
// only checked: a bar that doesn't add up to four beats is reported.

import { BEATS_PER_BAR } from './song';

export interface MelodyNote {
  /** Start in beats from the beginning of the song. */
  at: number;
  /** Length in beats. */
  len: number;
  /** MIDI key. */
  key: number;
}

export interface MelodyParse {
  notes: MelodyNote[];
  /** Tokens that aren't notes, and bars that don't add up. */
  problems: string[];
}

const LETTERS: Record<string, number> = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
const NOTE = /^([A-Ga-g])([#b]?)(-?\d)?(?::(\d+(?:\.\d+)?)(?:\/(\d+))?)?$/;
const REST = /^r(?::(\d+(?:\.\d+)?)(?:\/(\d+))?)?$/;

const lengthOf = (whole: string | undefined, divisor: string | undefined) =>
  whole === undefined ? 1 : Number(whole) / (divisor === undefined ? 1 : Number(divisor));

export function parseMelody(text: string): MelodyParse {
  const notes: MelodyNote[] = [];
  const problems: string[] = [];
  let at = 0;
  let octave = 4;
  let barStart = 0;
  let bar = 1;
  const checkBar = () => {
    const len = at - barStart;
    if (len > 0 && Math.abs(len - BEATS_PER_BAR) > 1e-6) problems.push(`Bar ${bar} has ${len} beats`);
    barStart = at;
    bar++;
  };
  for (const token of text.split(/\s+|(?=\|)|(?<=\|)/).filter(Boolean)) {
    if (token === '|') {
      checkBar();
      continue;
    }
    const rest = REST.exec(token);
    if (rest) {
      at += lengthOf(rest[1], rest[2]);
      continue;
    }
    const m = NOTE.exec(token);
    const len = m ? lengthOf(m[4], m[5]) : 0;
    if (!m || !(len > 0)) {
      problems.push(`Not a note: ${token}`);
      continue;
    }
    if (m[3] !== undefined) octave = Number(m[3]);
    const pc = LETTERS[m[1]!.toUpperCase()]! + (m[2] === '#' ? 1 : m[2] === 'b' ? -1 : 0);
    const key = (octave + 1) * 12 + pc;
    if (key >= 0 && key < 128) notes.push({ at, len, key });
    else problems.push(`Out of range: ${token}`);
    at += len;
  }
  checkBar();
  return { notes, problems };
}

/** A melody from notation that must be valid, for built-in songs. */
export function melody(text: string): MelodyNote[] {
  const { notes, problems } = parseMelody(text);
  if (problems.length) throw new Error(`Bad melody: ${problems.join('; ')}`);
  return notes;
}

/** The lowest and highest key, or null for no notes. */
export function melodyRange(notes: readonly MelodyNote[]): { low: number; high: number } | null {
  if (notes.length === 0) return null;
  let low = 127;
  let high = 0;
  for (const n of notes) {
    low = Math.min(low, n.key);
    high = Math.max(high, n.key);
  }
  return { low, high };
}
