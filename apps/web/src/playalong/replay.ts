// "Replay my solo": the notes of a solo run, recorded from the engine's
// `soloNote` events, and loaded back into the engine to play
// (crates/midzer-core/src/replay.rs).

import type { EngineCommand, TimedEvent } from '@midzer/audio-web';

/** Ticks per beat, as the engine's. */
export const PPQ = 480;
/** The solo's bars are four beats, as the engine's. */
export const TICKS_PER_BAR = 4 * PPQ;
/** The engine replays this many notes at most. */
export const MAX_REPLAY_NOTES = 1024;

/** A note of the solo: ticks from the run's first bar's 1, and whether the player played it or midzer. */
export interface TakeNote {
  tick: number;
  len: number;
  key: number;
  velocity: number;
  own: boolean;
}

/** A recorded solo, its notes in order, and how many bars it spans. */
export interface SoloTake {
  notes: TakeNote[];
  bars: number;
}

type SoloNoteEvent = Pick<Extract<TimedEvent, { type: 'soloNote' }>, 'key' | 'velocity' | 'bar' | 'tick' | 'own'>;

/** Bars wrap at 65536 in the events; a little before the first one counts as before. */
function barsSince(bar: number, first: number): number {
  const d = (((bar - first) % 65536) + 65536) % 65536;
  return d >= 32768 ? d - 65536 : d;
}

/** Records one run's solo from its `soloNote` events. */
export class TakeRecorder {
  private first: number | null = null;
  private notes: TakeNote[] = [];
  /** The notes sounding, by key. */
  private open = new Map<number, TakeNote>();
  private last = 0;

  note(e: SoloNoteEvent): void {
    this.first ??= e.bar;
    const tick = barsSince(e.bar, this.first) * TICKS_PER_BAR + e.tick;
    this.last = Math.max(this.last, tick);
    const sounding = this.open.get(e.key);
    if (sounding) {
      sounding.len = Math.max(1, tick - sounding.tick);
      this.open.delete(e.key);
    }
    if (e.velocity > 0) {
      const note = { tick, len: 0, key: e.key, velocity: e.velocity, own: e.own };
      this.notes.push(note);
      this.open.set(e.key, note);
    }
  }

  get size(): number {
    return this.notes.length;
  }

  /** The solo so far, notes still sounding ended at the last tick heard; null with no notes. */
  take(): SoloTake | null {
    if (this.notes.length === 0) return null;
    const notes = this.notes.map((n) => ({ ...n, len: this.open.get(n.key) === n ? Math.max(1, this.last - n.tick) : n.len }));
    // A note heard just before the first bar moves everything a bar on.
    const shift = Math.ceil(Math.max(0, -Math.min(...notes.map((n) => n.tick))) / TICKS_PER_BAR) * TICKS_PER_BAR;
    for (const n of notes) n.tick += shift;
    notes.sort((a, b) => a.tick - b.tick);
    const end = Math.max(...notes.map((n) => n.tick + n.len));
    return { notes, bars: Math.max(1, Math.ceil(end / TICKS_PER_BAR)) };
  }
}

/** The commands that load `take` into the engine for replaying. */
export function replayCommands(take: SoloTake): EngineCommand[] {
  return [
    { type: 'replayClear' },
    ...take.notes.slice(0, MAX_REPLAY_NOTES).map((n): EngineCommand => ({ type: 'replayNote', tick: n.tick, len: n.len, key: n.key, velocity: n.velocity })),
  ];
}

/** How much of the solo the player played, 0..1. */
export function ownShare(take: SoloTake): number {
  return take.notes.length === 0 ? 0 : take.notes.filter((n) => n.own).length / take.notes.length;
}
