// The song's key, for soloing in it: worked out from the chords heard (or
// a followed song's chart), weighted by how long each lasts. The solo plays
// the key's pentatonic, which sounds right over every chord of the key; the
// home note is the one key a beginner taps.

import { NOTE_NAMES, QUALITY_INTERVALS } from '@midzer/audio-web';
import type { ChordShape } from './levels';

export interface SongKey {
  /** The home note, 0 is C. */
  tonic: number;
  minor: boolean;
}

export interface WeightedChord {
  chord: ChordShape;
  /** How long it lasts, in any unit. */
  weight: number;
}

/** Krumhansl–Kessler key profiles, from the tonic up. */
const MAJOR = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR = [6.33, 2.68, 3.52, 5.38, 2.6, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];
/** A chord on the key's home note, of its kind, counts this much more for the key. */
const HOME_CHORD = 0.15;

const MAJOR_PENTATONIC = [0, 2, 4, 7, 9];
const MINOR_PENTATONIC = [0, 3, 5, 7, 10];

const isMinor = (q: number) => (QUALITY_INTERVALS[q] ?? []).includes(3);

/** How well each of the 24 keys fits the chords: majors 0–11 by tonic, then minors. */
export function keyScores(chords: readonly WeightedChord[]): number[] {
  const profile = new Array<number>(12).fill(0);
  for (const { chord, weight } of chords) {
    for (const iv of QUALITY_INTERVALS[chord.quality] ?? []) profile[(chord.root + iv) % 12]! += weight * (iv === 0 ? 2 : 1);
  }
  const total = chords.reduce((a, c) => a + c.weight, 0);
  const scores: number[] = [];
  for (const minor of [false, true]) {
    for (let tonic = 0; tonic < 12; tonic++) {
      const shape = minor ? MINOR : MAJOR;
      let score = correlation(profile, (pc) => shape[(pc - tonic + 12) % 12]!);
      // Relative keys share their notes; the chords on the home note tell them apart.
      if (total > 0) {
        const home = chords.filter((c) => c.chord.root % 12 === tonic && isMinor(c.chord.quality) === minor).reduce((a, c) => a + c.weight, 0);
        score += HOME_CHORD * (home / total);
      }
      scores.push(score);
    }
  }
  return scores;
}

function correlation(xs: readonly number[], y: (pc: number) => number): number {
  const ys = xs.map((_, pc) => y(pc));
  const mean = (v: readonly number[]) => v.reduce((a, b) => a + b, 0) / v.length;
  const [mx, my] = [mean(xs), mean(ys)];
  let [num, dx, dy] = [0, 0, 0];
  for (let i = 0; i < 12; i++) {
    num += (xs[i]! - mx) * (ys[i]! - my);
    dx += (xs[i]! - mx) ** 2;
    dy += (ys[i]! - my) ** 2;
  }
  return dx > 0 && dy > 0 ? num / Math.sqrt(dx * dy) : 0;
}

const keyAt = (i: number): SongKey => ({ tonic: i % 12, minor: i >= 12 });

/** The key the chords are most likely in, or null with no chords. */
export function keyOf(chords: readonly WeightedChord[]): SongKey | null {
  if (chords.length === 0) return null;
  const scores = keyScores(chords);
  return keyAt(scores.indexOf(Math.max(...scores)));
}

/** "A minor". */
export function keyName(key: SongKey): string {
  return `${NOTE_NAMES[key.tonic]} ${key.minor ? 'minor' : 'major'}`;
}

/** The key's pentatonic as a pitch-class bit set (bit 0 is C). */
export function pentatonic(key: SongKey): number {
  return bits((key.minor ? MINOR_PENTATONIC : MAJOR_PENTATONIC).map((iv) => key.tonic + iv));
}

/** A chord's tones as a pitch-class bit set. */
export function chordBits(chord: ChordShape | null): number {
  return chord ? bits((QUALITY_INTERVALS[chord.quality] ?? []).map((iv) => chord.root + iv)) : 0;
}

function bits(pcs: readonly number[]): number {
  return pcs.reduce((b, pc) => b | (1 << (pc % 12)), 0);
}

/** Another key must fit this much better than the one kept ... */
const SWITCH_MARGIN = 0.1;
/** ... for this long before the solo moves to it. */
const SWITCH_MS = 8000;

/**
 * The key, kept steady: the home note the player taps shouldn't wander with
 * every chord, so another key takes over only once it has fit clearly
 * better for a while.
 */
export class KeyFollower {
  private kept: SongKey | null = null;
  private rival: { index: number; sinceMs: number } | null = null;

  get key(): SongKey | null {
    return this.kept;
  }

  /** Takes the chords of the last while at `nowMs`; returns the key kept. */
  update(chords: readonly WeightedChord[], nowMs: number): SongKey | null {
    if (chords.length === 0) return this.kept;
    const scores = keyScores(chords);
    const best = scores.indexOf(Math.max(...scores));
    if (!this.kept) {
      this.kept = keyAt(best);
      return this.kept;
    }
    const keptIndex = this.kept.tonic + (this.kept.minor ? 12 : 0);
    if (best === keptIndex || scores[best]! < scores[keptIndex]! + SWITCH_MARGIN) {
      this.rival = null;
      return this.kept;
    }
    if (this.rival?.index !== best) this.rival = { index: best, sinceMs: nowMs };
    else if (nowMs - this.rival.sinceMs >= SWITCH_MS) {
      this.kept = keyAt(best);
      this.rival = null;
    }
    return this.kept;
  }

  /** A key known for sure (a song followed), kept until reset. */
  set(key: SongKey): void {
    this.kept = key;
    this.rival = null;
  }

  reset(): void {
    this.kept = null;
    this.rival = null;
  }
}
