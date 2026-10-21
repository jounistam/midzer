// Which keys are sounding right now, for 60 fps visuals. Kept outside React
// state on purpose: the keyboard canvas polls it every frame.
//
// Engine events arrive a little before they are heard (by the audio output
// latency), so they are queued and applied at their `timeMs`.

import { MIC_CHANNEL, partOfChannel, type TimedEvent } from '@midzer/audio-web';

export const live = {
  /** Velocity of each key the player is holding, 0 when up. */
  player: new Uint8Array(128),
  /** Number of band notes sounding on each key. */
  band: new Uint8Array(128),
  /** Keys Learn asks for next: 1 melody, 2 chord, 0 none. */
  hint: new Uint8Array(128),
  /** Microphone loudness 0..1 and the note it hears (-1 none), for the meter. */
  mic: { level: 0, key: -1 },
  /** Bumped on every change so renderers can skip unchanged frames. */
  version: 0,
};

interface Pending {
  timeMs: number;
  event: TimedEvent;
}

const queue: Pending[] = [];

export function enqueue(event: TimedEvent, timeMs: number): void {
  if (event.type === 'noteOn' || event.type === 'noteOff') queue.push({ timeMs, event });
}

/** Applies every queued note whose time has come. Call once per frame. */
export function flushLive(now = performance.now()): void {
  let applied = 0;
  while (applied < queue.length && queue[applied]!.timeMs <= now) {
    apply(queue[applied]!.event);
    applied++;
  }
  if (applied > 0) {
    queue.splice(0, applied);
    live.version++;
  }
}

export function resetLive(): void {
  queue.length = 0;
  live.player.fill(0);
  live.band.fill(0);
  live.version++;
}

function apply(event: TimedEvent): void {
  if (event.type !== 'noteOn' && event.type !== 'noteOff') return;
  // The microphone's notes show like keys the player holds.
  const part = event.channel === MIC_CHANNEL ? 'player' : partOfChannel(event.channel);
  if (part === null || part === 'drums') return;
  const on = event.type === 'noteOn';
  if (part === 'player') {
    live.player[event.key] = on ? event.velocity : 0;
  } else {
    const n = live.band[event.key]!;
    live.band[event.key] = on ? Math.min(n + 1, 255) : Math.max(n - 1, 0);
  }
}
