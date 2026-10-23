// The chords heard in a live source, as a timeline in the player's time. The
// listener sends the chords of its last 10 s about once a second; each
// batch replaces what the earlier ones said about that stretch. Its newest
// chord is about a second old when it arrives, so it carries on until the
// next batch says otherwise; once the song's loop is clear, the batch also
// says which chords come next (predicted), and those show on time.

import type { LiveChordSpan } from '@midzer/audio-web';
import { voicing, type AlongLevel, type ChordShape } from './levels';

export interface HeardChord {
  fromMs: number;
  toMs: number;
  /** "Am7", or null for no chord. */
  name: string | null;
  chord: ChordShape | null;
  predicted: boolean;
}

export interface ChordNow {
  current: string | null;
  /** The chords before it, newest first, each once in a row. */
  before: string[];
  /** The next chord the song's loop says is coming, and in how many ms. */
  next: { name: string; inMs: number } | null;
}

/** How long the newest chord carries on past what was heard of it. */
const CARRY_MS = 4000;
const KEEP_MS = 30_000;

export class ChordTimeline {
  private spans: HeardChord[] = [];

  /** Takes a batch from the listener, shifted by `shiftMs` into the player's time. */
  add(batch: LiveChordSpan[], shiftMs: number): void {
    if (batch.length === 0) return;
    const from = batch[0]!.fromMs + shiftMs;
    const kept = this.spans.filter((s) => s.fromMs < from).map((s) => ({ ...s, toMs: Math.min(s.toMs, from) }));
    const fresh = batch.map((s) => ({ fromMs: s.fromMs + shiftMs, toMs: s.toMs + shiftMs, name: s.chord?.name ?? null, chord: s.chord ? { root: s.chord.root, quality: s.chord.quality } : null, predicted: s.predicted }));
    const merged: HeardChord[] = [];
    for (const s of [...kept, ...fresh]) {
      const last = merged[merged.length - 1];
      if (last && last.name === s.name && last.predicted === s.predicted && s.fromMs - last.toMs < 1) last.toMs = Math.max(last.toMs, s.toMs);
      else merged.push(s);
    }
    const newest = merged[merged.length - 1]!.toMs;
    this.spans = merged.filter((s) => s.toMs > newest - KEEP_MS);
  }

  clear(): void {
    this.spans = [];
  }

  /** The spans between `fromMs` and `toMs`, the newest heard one carried on to `nowMs` as `at` does. */
  window(fromMs: number, toMs: number, nowMs: number): HeardChord[] {
    const out = this.spans.filter((s) => s.toMs > fromMs && s.fromMs < toMs).map((s) => ({ ...s }));
    const last = out[out.length - 1];
    if (last && last.toMs < nowMs && nowMs - last.toMs <= CARRY_MS) last.toMs = nowMs;
    return out;
  }

  at(nowMs: number, trail = 3): ChordNow {
    const spans = this.spans;
    let i = spans.findIndex((s) => s.fromMs <= nowMs && nowMs < s.toMs);
    if (i < 0) {
      const last = spans[spans.length - 1];
      if (!last || nowMs < last.toMs || nowMs - last.toMs > CARRY_MS) return { current: null, before: [], next: null };
      i = spans.length - 1;
    }
    const current = spans[i]!.name;
    const before: string[] = [];
    for (let j = i - 1; j >= 0 && before.length < trail; j--) {
      const name = spans[j]!.name;
      if (name !== null && name !== current && name !== before[before.length - 1]) before.push(name);
    }
    const coming = spans.slice(i + 1).find((s) => s.predicted && s.name !== null && s.name !== current);
    const next = coming ? { name: coming.name!, inMs: coming.fromMs - nowMs } : null;
    return { current, before, next };
  }
}

/** Keys to light for the player: 1 left hand, 2 right hand, 3 the next chord's. */
export type KeyHint = 1 | 2 | 3;

/** Within this of the next chord, its keys are the ones to play. */
const SWITCH_MS = 150;

/**
 * The keys of the chord heard now at `level`, and, once the song's loop says
 * a new chord comes within `leadMs`, that one's keys too, to move towards.
 */
export function keyHints(spans: readonly HeardChord[], nowMs: number, level: AlongLevel, leadMs: number): Map<number, KeyHint> {
  const hints = new Map<number, KeyHint>();
  const i = spans.findIndex((s) => s.fromMs <= nowMs && nowMs < s.toMs);
  let now = i >= 0 ? spans[i]!.chord : null;
  const coming = spans.slice(i + 1).find((s) => s.predicted && s.chord !== null && s.name !== spans[i]?.name && s.fromMs > nowMs);
  let next = coming && coming.fromMs - nowMs <= leadMs ? coming.chord : null;
  if (next && coming!.fromMs - nowMs <= SWITCH_MS) [now, next] = [next, null];
  if (next) for (const k of [...voicing(next, level).left, ...voicing(next, level).right]) hints.set(k, 3);
  if (now) {
    const v = voicing(now, level);
    for (const k of v.left) hints.set(k, 1);
    for (const k of v.right) hints.set(k, 2);
  }
  return hints;
}

/** Spans this close count as one chord in a row. */
const JOIN_MS = 50;

/**
 * The chords in `spans` as they fall onto the keys: a chord heard and then
 * predicted to go on is one, and so is a chord over a gap too short to play.
 * No-chord spans are left out. `predicted` when none of it was heard yet.
 */
export function chordBlocks(spans: readonly HeardChord[]): HeardChord[] {
  const out: HeardChord[] = [];
  for (const s of spans) {
    if (s.chord === null) continue;
    const last = out[out.length - 1];
    if (last && last.name === s.name && s.fromMs - last.toMs < JOIN_MS) {
      last.toMs = Math.max(last.toMs, s.toMs);
      last.predicted &&= s.predicted;
    } else out.push({ ...s });
  }
  return out;
}

/** A chord's edge this close to a beat (as a share of the beat) moves onto it. */
const SNAP = 0.4;

/**
 * The spans with their edges moved onto the beats the player sees (the beat
 * at `beatMs`, every `periodMs`), so chords change on the beat lines: the
 * listener's grid and the one shown can be a few ms apart. Spans left
 * shorter than a tenth of a beat are dropped.
 */
export function snapToBeats(spans: readonly HeardChord[], beatMs: number, periodMs: number): HeardChord[] {
  if (!(periodMs > 0) || !Number.isFinite(beatMs)) return spans.map((s) => ({ ...s }));
  const snap = (ms: number) => {
    const at = beatMs + Math.round((ms - beatMs) / periodMs) * periodMs;
    return Math.abs(ms - at) < SNAP * periodMs ? at : ms;
  };
  return spans.map((s) => ({ ...s, fromMs: snap(s.fromMs), toMs: snap(s.toMs) })).filter((s) => s.toMs - s.fromMs > periodMs / 10);
}
