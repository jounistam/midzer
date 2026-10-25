// Accompanying while playing along: the levels, from tapping the chord's root
// while midzer plays a keys pattern, and the patterns to pick from. The
// pattern itself is played by the engine (crates/midzer-core/src/accomp.rs).

import type { ChordShape } from './levels';
import { TAP_HINT } from './solo';

export type AccompLevel = 1 | 2;

export const ACCOMP_LEVELS: { level: AccompLevel; name: string; detail: string }[] = [
  { level: 1, name: 'Tap the root', detail: "Tap the lit key, the chord's root, and midzer plays the pattern for two bars. Tap faster for a busier pattern, harder for louder." },
  { level: 2, name: 'Hold the root', detail: 'Hold the lit root and the pattern plays while you hold it. Press it again faster for a busier pattern.' },
];

export function parseAccompLevel(value: unknown): AccompLevel {
  return ACCOMP_LEVELS.some((l) => l.level === value) ? (value as AccompLevel) : 1;
}

export type AccompPattern = 'arpeggio' | 'broken' | 'stabs' | 'ballad' | 'bounce';

/** In the engine's order: the index is what `setAccomp` sends. */
export const ACCOMP_PATTERNS: { id: AccompPattern; name: string; detail: string }[] = [
  { id: 'arpeggio', name: 'Arpeggio', detail: 'The chord one note at a time, up and down, over the root.' },
  { id: 'broken', name: 'Broken chord', detail: 'Low, high, middle, high: the classic left hand of a piano piece.' },
  { id: 'stabs', name: 'Pop stabs', detail: 'Short chords on the beat and off it, over a moving bass.' },
  { id: 'ballad', name: 'Ballad', detail: 'A long low root and the chord rippling gently above it.' },
  { id: 'bounce', name: 'Bounce', detail: 'Bass on the beat, chords on the off-beats, as in stride piano.' },
];

export function parseAccompPattern(value: unknown): AccompPattern {
  return ACCOMP_PATTERNS.some((p) => p.id === value) ? (value as AccompPattern) : 'arpeggio';
}

export function patternIndex(pattern: AccompPattern): number {
  return Math.max(0, ACCOMP_PATTERNS.findIndex((p) => p.id === pattern));
}

/** The root to tap from middle C up, or from the C below it beside a solo above `split`. */
export function accompRoot(chord: ChordShape, split = 0): number {
  return (split > 0 ? 48 : 60) + (chord.root % 12);
}

/** The keys lit at `level` for the chord now: its root to tap. */
export function accompHints(level: AccompLevel, chord: ChordShape | null, split = 0): Map<number, number> {
  const hints = new Map<number, number>();
  if (!chord) return hints;
  if (level <= 2) hints.set(accompRoot(chord, split), TAP_HINT);
  return hints;
}
