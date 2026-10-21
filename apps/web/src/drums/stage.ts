// What the drummer's stage display says: the chord and the next change,
// the section and where it goes, the count-in, and whether the hits rush or drag.

import { VARIATION_NAMES, type BandState, type DrumPiece } from '@midzer/audio-web';
import { PIECES } from './kit';

/** The next chord after song beat `at` in `beats`, and how many beats away it is. Wraps for loops. */
export function nextChord(beats: readonly (string | null)[], at: number, loop = false): { name: string; in: number } | null {
  const n = beats.length;
  if (n === 0) return null;
  const current = beats[((at % n) + n) % n] ?? null;
  for (let i = 1; i < (loop ? n : n - at); i++) {
    const beat = beats[(at + i) % n] ?? null;
    if (beat !== current) return { name: beat ?? 'N.C.', in: i };
  }
  return null;
}

export interface StageSection {
  /** What plays now: "A", "Fill", "Intro"… */
  now: string;
  /** Where it goes, e.g. "B"; null when nothing is queued. */
  next: string | null;
  /** What the drummer can do about it. */
  hint: string;
}

/** The section as the stage shows it, read from about two metres away. */
export function stageSection(band: BandState): StageSection {
  const v = VARIATION_NAMES[band.variation] ?? 'A';
  const next = VARIATION_NAMES[band.nextVariation] ?? 'A';
  if (!band.playing) return { now: next, next: null, hint: '' };
  switch (band.section) {
    case 'intro':
      return { now: 'Intro', next, hint: '' };
    case 'fill':
      return { now: 'Fill', next: next === v ? null : next, hint: '' };
    case 'ending':
      return { now: 'Ending', next: null, hint: '' };
    case 'rest':
      return { now: 'Rest', next: null, hint: 'Play to bring the band back' };
    default:
      if (band.cued) return { now: v, next, hint: `Crash on the 1 for ${next}` };
      if (band.endingPending) return { now: v, next: 'End', hint: '' };
      if (band.fillPending) return { now: v, next: next === v ? 'Fill' : next, hint: '' };
      return { now: v, next: null, hint: 'Fill, then crash on the 1' };
  }
}

/** Which pieces count the band in. */
export type CountInChoice = 'hiHat' | 'stick' | 'any' | 'off';

export const COUNT_IN_OPTIONS: { choice: CountInChoice; label: string; pieces: DrumPiece[] }[] = [
  { choice: 'hiHat', label: 'Hi-hat', pieces: ['hiHatClosed', 'hiHatPedal', 'hiHatOpen'] },
  { choice: 'stick', label: 'Sticks', pieces: ['sideStick', 'snareRim'] },
  { choice: 'any', label: 'Any drum', pieces: [...PIECES] },
  { choice: 'off', label: 'Off', pieces: [] },
];

export function countInPieces(choice: CountInChoice): DrumPiece[] {
  return COUNT_IN_OPTIONS.find((o) => o.choice === choice)?.pieces ?? [];
}

export function countInLabel(choice: CountInChoice): string {
  switch (choice) {
    case 'hiHat':
      return 'Count in on the hi-hat: four even hits';
    case 'stick':
      return 'Count in with stick clicks: four even hits';
    case 'any':
      return 'Count in on any drum: four even hits';
    case 'off':
      return 'Press Play to start';
  }
}

/** How quickly the meter follows each hit. */
const TIMING_SMOOTHING = 0.2;
/** Closer to the grid than this, on average, counts as in the pocket. */
export const POCKET_MS = 8;

/** A running average of how far the drummer's hits land off the band's grid. */
export class TimingMeter {
  /** Smoothed offset in ms; negative is ahead of the band (rushing). */
  average = 0;
  hits = 0;

  push(ms: number): void {
    this.average = this.hits === 0 ? ms : this.average + TIMING_SMOOTHING * (ms - this.average);
    this.hits++;
  }

  reset(): void {
    this.average = 0;
    this.hits = 0;
  }
}

export function timingVerdict(averageMs: number, hits: number): string {
  if (hits < 4) return 'Listening…';
  const ms = Math.round(Math.abs(averageMs));
  if (ms < POCKET_MS) return 'In the pocket';
  return averageMs < 0 ? `Rushing ${ms} ms` : `Dragging ${ms} ms`;
}
