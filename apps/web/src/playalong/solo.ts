// Improvising while playing along: the levels, from tapping one key while
// midzer solos up to playing every note yourself, and the keys each lights.
// The solo itself is played by the engine (crates/midzer-core/src/solo.rs).

import { pentatonic, type SongKey } from './scale';

/** Play the chords heard, or solo over them. */
export type AlongMode = 'chords' | 'solo';

export type SoloLevel = 1 | 2 | 3 | 4 | 5;

export const SOLO_LEVELS: { level: SoloLevel; name: string; detail: string }[] = [
  { level: 1, name: 'One key', detail: "Tap the lit key, the song's home note, and midzer solos. Tap faster for more, harder for louder; hold it for a long note." },
  { level: 2, name: 'High and low', detail: 'Tap one of the three lit keys to steer: the home note keeps the solo low, the fifth in the middle, the home note an octave up takes it high.' },
  { level: 3, name: 'Your notes, our groove', detail: 'Play the lit keys, the song’s scale; the bright ones fit the chord now. Notes off the scale snap onto it, and midzer fills the gaps.' },
  { level: 4, name: 'Safety net', detail: 'Play every note yourself. Notes off the scale still snap onto it, and your timing is judged.' },
  { level: 5, name: 'On your own', detail: 'Play freely. Each note shows whether it fits the chord, the scale or neither, with your timing.' },
];

export const DEFAULT_SOLO_LEVEL: SoloLevel = 1;

export function parseSoloLevel(value: unknown): SoloLevel {
  return SOLO_LEVELS.some((l) => l.level === value) ? (value as SoloLevel) : DEFAULT_SOLO_LEVEL;
}

export function parseAlongMode(value: unknown): AlongMode {
  return value === 'solo' ? 'solo' : 'chords';
}

/** The key to tap: the home note from middle C up, as the engine's `Solo::home`. */
export function homeKey(key: SongKey): number {
  return 60 + key.tonic;
}

/** Hint values (see `live.hint`): a key to tap, a note of the scale, a tone of the chord heard now. */
export const TAP_HINT = 4;
export const SCALE_HINT = 5;
export const CHORD_TONE_HINT = 6;

/** The levels where the space bar taps the home note. */
export const SPACE_TAPS_TO = 2;

/** The keys lit at `level` once the song's key is known; `chord` is the chord heard as pitch-class bits. */
export function soloHints(level: SoloLevel, key: SongKey | null, chord = 0): Map<number, number> {
  const hints = new Map<number, number>();
  if (!key) return hints;
  const home = homeKey(key);
  if (level === 1) hints.set(home, TAP_HINT);
  if (level === 2) for (const k of [home, home + 7, home + 12]) hints.set(k, TAP_HINT);
  if (level === 3 || level === 4) {
    // Two octaves from the home note up.
    const scale = pentatonic(key);
    for (let k = home; k <= home + 24; k++) {
      if (scale & (1 << k % 12)) hints.set(k, chord & (1 << k % 12) ? CHORD_TONE_HINT : SCALE_HINT);
    }
  }
  return hints;
}

/** How a note fits the harmony. */
export type NoteFit = 'chord' | 'scale' | 'outside';

/** Whether `key` is a tone of the chord, in the scale, or neither (pitch-class bits). */
export function noteFit(key: number, scale: number, chord: number): NoteFit {
  const bit = 1 << key % 12;
  return chord & bit ? 'chord' : scale & bit ? 'scale' : 'outside';
}

/** The player's notes so far by fit, and the last one's. */
export interface FitCount {
  chord: number;
  scale: number;
  outside: number;
  last: NoteFit | null;
}

export const NO_FIT: FitCount = { chord: 0, scale: 0, outside: 0, last: null };

/** The feel meter's words, calm to fire. */
export function feelWord(energy: number): string {
  if (energy < 0.06) return 'Resting';
  if (energy < 0.35) return 'Calm';
  if (energy < 0.6) return 'Grooving';
  if (energy < 0.85) return 'Burning';
  return 'On fire';
}
