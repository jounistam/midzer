// Improvising while playing along: the levels, from tapping one key while
// midzer solos up to playing every note yourself, and the keys each lights.
// The solo itself is played by the engine (crates/midzer-core/src/solo.rs).

import type { SongKey } from './scale';

/** Play the chords heard, or solo over them. */
export type AlongMode = 'chords' | 'solo';

export type SoloLevel = 1 | 2 | 3 | 4 | 5;

export const SOLO_LEVELS: { level: SoloLevel; name: string; detail: string }[] = [
  { level: 1, name: 'One key', detail: "Tap the lit key, the song's home note, and midzer solos. Tap faster for more, harder for louder; hold it for a long note." },
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

/** A key's hint value (see `live.hint`): the key to tap. */
export const TAP_HINT = 4;

/** The keys lit at `level` once the song's key is known. */
export function soloHints(_level: SoloLevel, key: SongKey | null): Map<number, number> {
  const hints = new Map<number, number>();
  if (key) hints.set(homeKey(key), TAP_HINT);
  return hints;
}

/** The feel meter's words, calm to fire. */
export function feelWord(energy: number): string {
  if (energy < 0.06) return 'Resting';
  if (energy < 0.35) return 'Calm';
  if (energy < 0.6) return 'Grooving';
  if (energy < 0.85) return 'Burning';
  return 'On fire';
}
