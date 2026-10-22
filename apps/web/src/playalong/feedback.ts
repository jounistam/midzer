// Instant feedback while playing along: the last hits for the accuracy
// display, and a soft tick when a hit is clearly off, so a player who is
// watching their instrument still hears it. Early hits tick higher, late ones
// lower; a wrong chord gets a low double tick.

import { OFF_MS, ON_TIME_MS } from '../drums/judging';

export type MissSound = 'off' | 'soft' | 'loud';

export const MISS_SOUND_OPTIONS: { value: MissSound; label: string }[] = [
  { value: 'off', label: 'Off' },
  { value: 'soft', label: 'Soft' },
  { value: 'loud', label: 'Loud' },
];

export type Miss = 'early' | 'late' | 'chord';

/** What, if anything, was off about a judged hit: the chord first, then the timing. */
export function missOf(ms: number, chord: boolean | null | undefined): Miss | null {
  if (chord === false) return 'chord';
  if (Math.abs(ms) <= OFF_MS) return null;
  return ms < 0 ? 'early' : 'late';
}

/** How a hit landed, for colouring it: on time, a little off, or clearly off (or a wrong chord). */
export type Grade = 'on' | 'near' | 'off';

export function gradeOf(ms: number, chord?: boolean | null): Grade {
  if (chord === false || Math.abs(ms) > OFF_MS) return 'off';
  return Math.abs(ms) <= ON_TIME_MS ? 'on' : 'near';
}

export interface RecentHit {
  ms: number;
  chord: boolean | null;
  /** performance.now() when it was judged. */
  atMs: number;
}

const KEPT = 32;

/** The last hits judged in this run, newest last. Polled by the display every frame. */
export class RecentHits {
  hits: RecentHit[] = [];
  /** Bumped on every hit, so a renderer can tell something new came in. */
  count = 0;

  push(ms: number, chord: boolean | null, atMs = performance.now()): void {
    this.hits.push({ ms, chord, atMs });
    if (this.hits.length > KEPT) this.hits.shift();
    this.count++;
  }

  reset(): void {
    this.hits = [];
    this.count = 0;
  }

  /** The share of the last `n` hits that were on time (and the right chord), or null before any. */
  onTime(n = 16): number | null {
    const last = this.hits.slice(-n);
    if (last.length === 0) return null;
    return last.filter((h) => gradeOf(h.ms, h.chord) === 'on').length / last.length;
  }
}

const GAIN: Record<MissSound, number> = { off: 0, soft: 0.12, loud: 0.35 };
const PITCH: Record<Miss, number> = { early: 1568, late: 784, chord: 220 };

let context: AudioContext | null = null;

/** A short, dry tick for a miss. Its own small AudioContext, so it works the same over the web engine and the desktop's native audio. */
export function playMiss(miss: Miss, sound: MissSound): void {
  if (sound === 'off' || typeof AudioContext === 'undefined') return;
  try {
    context ??= new AudioContext({ latencyHint: 'interactive' });
    if (context.state === 'suspended') void context.resume();
    const t = context.currentTime;
    const blips = miss === 'chord' ? [0, 0.07] : [0];
    for (const offset of blips) {
      const osc = context.createOscillator();
      const gain = context.createGain();
      osc.type = miss === 'chord' ? 'square' : 'triangle';
      osc.frequency.value = PITCH[miss];
      gain.gain.setValueAtTime(0, t + offset);
      gain.gain.linearRampToValueAtTime(GAIN[sound], t + offset + 0.002);
      gain.gain.exponentialRampToValueAtTime(0.0001, t + offset + 0.05);
      osc.connect(gain).connect(context.destination);
      osc.start(t + offset);
      osc.stop(t + offset + 0.06);
    }
  } catch {
    // No audio for feedback: the display still shows the miss.
  }
}
