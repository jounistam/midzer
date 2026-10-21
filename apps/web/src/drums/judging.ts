// How a drummer played against a recording: each hit arrives from the engine
// placed on the recording's grid (the `judged` event), and this sums a run up
// as overall accuracy, a histogram of offsets, accuracy bar by bar and what
// each part of the kit tends to do ("Snare: 12 ms late on the backbeat").

import type { DrumPiece } from '@midzer/audio-web';

export interface JudgedHit {
  /** From the nearest sixteenth; negative is early. */
  ms: number;
  piece: DrumPiece;
  /** 0..15 within the bar. */
  sixteenth: number;
  bar: number;
}

/** A hit this close to the grid counts as on time. */
export const ON_TIME_MS = 15;
/** Hits further off than this are left out of averages: more likely a different note than a late one. */
const OUTLIER_MS = 60;
/** A part's lean worth pointing out, and the hits it takes to say so. */
const LEAN_MS = 8;
const LEAN_HITS = 4;
export const HISTOGRAM_STEP_MS = 5;
export const HISTOGRAM_RANGE_MS = 50;

export type PartId = 'kick' | 'snare' | 'hiHat' | 'ride' | 'toms' | 'cymbals';

const PARTS: { id: PartId; label: string; pieces: DrumPiece[] }[] = [
  { id: 'kick', label: 'Kick', pieces: ['kick'] },
  { id: 'snare', label: 'Snare', pieces: ['snare', 'snareRim', 'sideStick'] },
  { id: 'hiHat', label: 'Hi-hat', pieces: ['hiHatClosed', 'hiHatOpen', 'hiHatPedal'] },
  { id: 'ride', label: 'Ride', pieces: ['ride', 'rideBell'] },
  { id: 'toms', label: 'Toms', pieces: ['tom1', 'tom2', 'tom3', 'tom4'] },
  { id: 'cymbals', label: 'Crashes', pieces: ['crash', 'crash2', 'splash', 'china'] },
];

/** Where in the bar a hit falls, for saying where a part leans. */
type Spot = 'the 1 and 3' | 'the backbeat' | 'the off-beats' | 'the sixteenths';

function spotOf(sixteenth: number): Spot {
  if (sixteenth % 2 === 1) return 'the sixteenths';
  if (sixteenth % 4 === 2) return 'the off-beats';
  return sixteenth % 8 === 4 ? 'the backbeat' : 'the 1 and 3';
}

export interface PartSummary {
  id: PartId;
  label: string;
  hits: number;
  /** Average offset; negative is early. */
  meanMs: number;
  /** Share of hits within ON_TIME_MS. */
  onTime: number;
  /** Where the part leans the most, if it clearly does: e.g. "late on the backbeat". */
  lean: string | null;
}

export interface BarSummary {
  bar: number;
  hits: number;
  onTime: number;
}

export interface TimingSummary {
  hits: number;
  onTime: number;
  meanMs: number;
  /** Standard deviation: how much the hits scatter. */
  spreadMs: number;
  /** Counts per HISTOGRAM_STEP_MS bin from -HISTOGRAM_RANGE_MS; the outer bins take everything beyond. */
  histogram: number[];
  bars: BarSummary[];
  parts: PartSummary[];
}

const mean = (xs: number[]) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : 0);
const onTimeShare = (xs: number[]) => (xs.length ? xs.filter((x) => Math.abs(x) <= ON_TIME_MS).length / xs.length : 0);

/** "12 ms late", "8 ms early" or "on time". */
export function describeOffset(ms: number): string {
  const r = Math.round(ms);
  if (Math.abs(r) < 3) return 'on time';
  return `${Math.abs(r)} ms ${r > 0 ? 'late' : 'early'}`;
}

function lean(hits: JudgedHit[]): string | null {
  const bySpot = new Map<Spot, number[]>();
  for (const h of hits) {
    if (Math.abs(h.ms) > OUTLIER_MS) continue;
    const spot = spotOf(h.sixteenth);
    bySpot.set(spot, [...(bySpot.get(spot) ?? []), h.ms]);
  }
  let best: { spot: Spot; ms: number } | null = null;
  for (const [spot, ms] of bySpot) {
    const m = mean(ms);
    if (ms.length >= LEAN_HITS && Math.abs(m) >= LEAN_MS && (!best || Math.abs(m) > Math.abs(best.ms))) best = { spot, ms: m };
  }
  return best ? `${describeOffset(best.ms)} on ${best.spot}` : null;
}

export function summarize(hits: readonly JudgedHit[]): TimingSummary {
  const all = hits.map((h) => h.ms);
  const kept = all.filter((ms) => Math.abs(ms) <= OUTLIER_MS);
  const m = mean(kept);
  const spread = Math.sqrt(mean(kept.map((x) => (x - m) ** 2)));

  const bins = (2 * HISTOGRAM_RANGE_MS) / HISTOGRAM_STEP_MS;
  const histogram = new Array<number>(bins).fill(0);
  for (const ms of all) {
    const i = Math.floor((ms + HISTOGRAM_RANGE_MS) / HISTOGRAM_STEP_MS);
    histogram[Math.min(bins - 1, Math.max(0, i))]!++;
  }

  const byBar = new Map<number, number[]>();
  for (const h of hits) byBar.set(h.bar, [...(byBar.get(h.bar) ?? []), h.ms]);
  const bars = [...byBar.entries()].sort((a, b) => a[0] - b[0]).map(([bar, ms]) => ({ bar, hits: ms.length, onTime: onTimeShare(ms) }));

  const parts = PARTS.flatMap(({ id, label, pieces }) => {
    const mine = hits.filter((h) => pieces.includes(h.piece));
    if (mine.length === 0) return [];
    const ms = mine.map((h) => h.ms);
    return [{ id, label, hits: mine.length, meanMs: mean(ms.filter((x) => Math.abs(x) <= OUTLIER_MS)), onTime: onTimeShare(ms), lean: lean(mine) }];
  });

  return { hits: hits.length, onTime: onTimeShare(all), meanMs: m, spreadMs: spread, histogram, bars, parts };
}
