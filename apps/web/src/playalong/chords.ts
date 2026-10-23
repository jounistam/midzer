// The chords heard in a live source, as a timeline in the player's time. The
// listener sends the chords of its last 10 s about once a second; each
// batch replaces what the earlier ones said about that stretch. Its newest
// chord is about a second old when it arrives, so it carries on until the
// next batch says otherwise; once the song's loop is clear, the batch also
// says which chords come next (predicted), and those show on time.

import type { LiveChordSpan } from '@midzer/audio-web';

export interface HeardChord {
  fromMs: number;
  toMs: number;
  /** "Am7", or null for no chord. */
  name: string | null;
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
    const fresh = batch.map((s) => ({ fromMs: s.fromMs + shiftMs, toMs: s.toMs + shiftMs, name: s.chord?.name ?? null, predicted: s.predicted }));
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
