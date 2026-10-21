// Gentle scoring for Learn. The judge matches the player's notes against the
// targets and grades each one.
//
// Playing in time (wait mode off), a target is graded by how far from its
// beat it was played; one not played by the end of its window is missed.
// In wait mode the band stops at each target until it is played, so timing
// doesn't count: a target is perfect when played without a wrong note first.
//
// A chord counts when all its pitch classes sound, in any octave or
// inversion, and at least one of its notes was struck for it: holding the
// last chord through a bar line doesn't play the next one.

import type { Target } from './targets';

export type Grade = 'perfect' | 'good' | 'ok' | 'miss';
/** A target's state in the current pass; skipped ones (jumped over) don't count. */
export type TargetState = 'open' | Grade | 'skipped';

export const WINDOW_MS = { perfect: 70, good: 140, ok: 280 } as const;
/** Wait mode: notes this many beats before the next target already count for it. */
export const WAIT_EARLY_BEATS = 0.5;
const POINTS: Record<Grade, number> = { perfect: 100, good: 75, ok: 50, miss: 0 };
/** A wrong note costs a quarter of a target. */
const WRONG_COST = 25;

export interface Judgement {
  target: Target;
  grade: Grade;
  /** How early (<0) or late (>0) it was played, in ms; null in wait mode or when missed. */
  offsetMs: number | null;
}

export interface Score {
  perfect: number;
  good: number;
  ok: number;
  miss: number;
  wrong: number;
  streak: number;
  bestStreak: number;
}

export interface JudgeOptions {
  wait: boolean;
  /** Melody notes count in any octave, e.g. for the computer keyboard's one octave. */
  anyOctave: boolean;
}

export const emptyScore = (): Score => ({ perfect: 0, good: 0, ok: 0, miss: 0, wrong: 0, streak: 0, bestStreak: 0 });

/** 0..1: points for the graded targets, less a little for wrong notes. */
export function accuracy(score: Score): number {
  const graded = score.perfect + score.good + score.ok + score.miss;
  const max = graded * POINTS.perfect + score.wrong * WRONG_COST;
  if (max === 0) return 0;
  return (score.perfect * POINTS.perfect + score.good * POINTS.good + score.ok * POINTS.ok) / max;
}

/** One to three stars; zero below half marks. */
export function starsFor(score: Score): 0 | 1 | 2 | 3 {
  const a = accuracy(score);
  return a >= 0.9 ? 3 : a >= 0.75 ? 2 : a >= 0.5 ? 1 : 0;
}

export class Judge {
  readonly states: TargetState[];
  readonly score: Score = emptyScore();
  private readonly held = new Map<number, number>(); // key → position it was struck at
  private last: number | null = null;
  /** Wait mode: a wrong note was played since the current target came up. */
  private fumbled = false;

  constructor(
    readonly targets: readonly Target[],
    private readonly options: JudgeOptions,
  ) {
    this.states = targets.map(() => 'open');
  }

  /** The first target not played yet in this pass. */
  nextOpen(): Target | null {
    const i = this.states.indexOf('open');
    return i < 0 ? null : this.targets[i]!;
  }

  /** The open targets due together at the next open beat. */
  current(): Target[] {
    const first = this.nextOpen();
    if (!first) return [];
    return this.targets.filter((t, i) => this.states[i] === 'open' && Math.abs(t.beat - first.beat) < 1e-6);
  }

  /** Whether every target of this pass has been graded or skipped. */
  done(): boolean {
    return !this.states.includes('open');
  }

  /** Targets before `beat` don't count in this pass, e.g. when starting later in the song. */
  skipBefore(beat: number): void {
    this.targets.forEach((t, i) => {
      if (this.states[i] === 'open' && t.beat < beat - 1e-6) this.states[i] = 'skipped';
    });
  }

  /** The player struck `key` at song position `pos`. */
  noteOn(key: number, pos: number, msPerBeat: number): Judgement[] {
    this.held.set(key, pos);
    const out = this.options.wait ? this.waitNoteOn(key, pos) : this.timedNoteOn(key, pos, msPerBeat);
    if (out.length === 0 && this.isWrong(key, pos, msPerBeat)) {
      this.score.wrong++;
      this.score.streak = 0;
      this.fumbled = true;
    }
    return out;
  }

  noteOff(key: number): void {
    this.held.delete(key);
  }

  /** The song reached `pos`: targets past their window are missed. Handles loops and jumps. */
  advance(pos: number, msPerBeat: number): Judgement[] {
    const last = this.last;
    this.last = pos;
    if (last !== null && pos < last - 0.5) {
      // Back to the loop start: its targets come round again.
      this.targets.forEach((t, i) => {
        if (t.beat >= pos - 1e-6) this.states[i] = 'open';
      });
      this.fumbled = false;
      return [];
    }
    if (last !== null && pos > last + 2) {
      // Jumped ahead: what was skipped doesn't count.
      this.targets.forEach((t, i) => {
        if (this.states[i] === 'open' && t.beat >= last && t.beat < pos - 1e-6) this.states[i] = 'skipped';
      });
    }
    const windowBeats = this.options.wait ? 1 : this.okBeats(msPerBeat);
    const out: Judgement[] = [];
    this.targets.forEach((t, i) => {
      if (this.states[i] === 'open' && pos - t.beat > windowBeats) out.push(this.grade(i, 'miss', null));
    });
    return out;
  }

  private okBeats(msPerBeat: number): number {
    return Math.min(WINDOW_MS.ok / msPerBeat, 0.5);
  }

  private matchesNote(t: Target, key: number): boolean {
    return this.options.anyOctave ? t.pcs[0] === key % 12 : t.keys[0] === key;
  }

  /** A chord sounds in full, with a note struck at or after `since`. */
  private chordSounds(t: Target, since: number): boolean {
    const pcs = new Set<number>();
    let fresh = false;
    for (const [key, at] of this.held) {
      pcs.add(key % 12);
      if (at >= since - 1e-6 && t.pcs.includes(key % 12)) fresh = true;
    }
    return fresh && t.pcs.every((pc) => pcs.has(pc));
  }

  private timedNoteOn(key: number, pos: number, msPerBeat: number): Judgement[] {
    const window = this.okBeats(msPerBeat);
    const out: Judgement[] = [];
    let best = -1;
    this.targets.forEach((t, i) => {
      if (this.states[i] !== 'open' || t.kind !== 'note' || Math.abs(pos - t.beat) > window) return;
      if (!this.matchesNote(t, key)) return;
      if (best < 0 || Math.abs(pos - t.beat) < Math.abs(pos - this.targets[best]!.beat)) best = i;
    });
    if (best >= 0) out.push(this.timed(best, pos, msPerBeat));
    this.targets.forEach((t, i) => {
      if (this.states[i] !== 'open' || t.kind !== 'chord' || Math.abs(pos - t.beat) > window) return;
      if (t.pcs.includes(key % 12) && this.chordSounds(t, t.beat - window)) out.push(this.timed(i, pos, msPerBeat));
    });
    return out;
  }

  private timed(i: number, pos: number, msPerBeat: number): Judgement {
    const offsetMs = (pos - this.targets[i]!.beat) * msPerBeat;
    const abs = Math.abs(offsetMs);
    const grade = abs <= WINDOW_MS.perfect ? 'perfect' : abs <= WINDOW_MS.good ? 'good' : 'ok';
    return this.grade(i, grade, offsetMs);
  }

  private waitNoteOn(key: number, pos: number): Judgement[] {
    const group = this.current();
    const first = group[0];
    if (!first || pos < first.beat - WAIT_EARLY_BEATS) return [];
    const out: Judgement[] = [];
    const grade = this.fumbled ? 'good' : 'perfect';
    for (const t of group) {
      const hit =
        t.kind === 'note' ? this.matchesNote(t, key) : t.pcs.includes(key % 12) && this.chordSounds(t, t.beat - WAIT_EARLY_BEATS);
      if (hit) out.push(this.grade(t.id, grade, null));
    }
    if (this.done() || this.current()[0] !== first) this.fumbled = false;
    return out;
  }

  /** A note that belongs to no target it could be meant for. */
  private isWrong(key: number, pos: number, msPerBeat: number): boolean {
    const near = this.options.wait
      ? this.current().filter((t) => pos >= t.beat - WAIT_EARLY_BEATS)
      : this.targets.filter((t, i) => this.states[i] === 'open' && Math.abs(pos - t.beat) <= this.okBeats(msPerBeat));
    if (this.options.wait && near.length === 0) return false; // too early to count either way
    // A note that belongs to a chord or note being played isn't wrong, even if it completes nothing yet.
    const graded = this.targets.some(
      (t, i) => this.states[i] !== 'open' && this.states[i] !== 'skipped' && this.states[i] !== 'miss' && Math.abs(pos - t.beat) <= 0.5 && t.pcs.includes(key % 12),
    );
    return !graded && !near.some((t) => (t.kind === 'note' ? this.matchesNote(t, key) : t.pcs.includes(key % 12)));
  }

  private grade(i: number, grade: Grade, offsetMs: number | null): Judgement {
    this.states[i] = grade;
    const s = this.score;
    s[grade]++;
    s.streak = grade === 'miss' ? 0 : s.streak + 1;
    s.bestStreak = Math.max(s.bestStreak, s.streak);
    return { target: this.targets[i]!, grade, offsetMs };
  }
}
