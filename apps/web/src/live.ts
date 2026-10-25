// Which keys are sounding right now, for 60 fps visuals. Kept outside React
// state on purpose: the keyboard canvas polls it every frame.
//
// Engine events arrive a little before they are heard (by the audio output
// latency), so they are queued and applied at their `timeMs`.

import { DRUM_PIECES, MIC_CHANNEL, partOfChannel, type TimedEvent } from '@midzer/audio-web';
import { TimingMeter } from './drums/stage';
import { RecentHits } from './playalong/feedback';

const PIECE_OF_NOTE = new Map<number, number>(DRUM_PIECES.map((p, i) => [p.note, i]));

export const live = {
  /** Velocity of each key the player is holding, 0 when up. */
  player: new Uint8Array(128),
  /** Number of band notes sounding on each key. */
  band: new Uint8Array(128),
  /** Keys Learn asks for next: 1 melody, 2 chord, 0 none. Playing along: 1 left hand, 2 right hand, 3 the next chord; soloing: 4 a key to tap, 5 a scale note, 6 a tone of the chord heard. */
  hint: new Uint8Array(128),
  /** When each drum piece was last hit (performance.now() ms), in engine piece order. */
  drumTime: new Float64Array(DRUM_PIECES.length).fill(-Infinity),
  /** Velocity of each piece's last hit. */
  drumVelocity: new Uint8Array(DRUM_PIECES.length),
  /** The hi-hat pedal (MIDI CC 4) as the kit last sent it, -1 before it has. */
  hiHat: -1,
  /** How far a drummer's hits land off the band's grid, smoothed. */
  timing: new TimingMeter(),
  /** Hits judged against a recording or live track in this run, for the accuracy display. */
  hits: new RecentHits(),
  /** Microphone loudness 0..1 and the note it hears (-1 none), for the meter. */
  mic: { level: 0, key: -1 },
  /** The last beat of a live source the player heard (performance.now() ms) and its place in the bar. */
  beatClock: { ms: -Infinity, beat: 0 },
  /** The last song beat heard (performance.now() ms) and its number from the start of the song. */
  songClock: { ms: -Infinity, beat: 0 },
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
    apply(queue[applied]!.event, queue[applied]!.timeMs);
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
  live.drumTime.fill(-Infinity);
  live.version++;
}

function apply(event: TimedEvent, timeMs: number): void {
  if (event.type !== 'noteOn' && event.type !== 'noteOff') return;
  // The microphone's notes show like keys the player holds.
  const part = event.channel === MIC_CHANNEL ? 'player' : partOfChannel(event.channel);
  if (part === null || part === 'drums') return;
  const on = event.type === 'noteOn';
  if (part === 'playerDrums') {
    const piece = PIECE_OF_NOTE.get(event.key);
    if (on && piece !== undefined) {
      live.drumTime[piece] = timeMs;
      live.drumVelocity[piece] = event.velocity;
    }
  } else if (part === 'player') {
    live.player[event.key] = on ? event.velocity : 0;
  } else {
    const n = live.band[event.key]!;
    live.band[event.key] = on ? Math.min(n + 1, 255) : Math.max(n - 1, 0);
  }
}
