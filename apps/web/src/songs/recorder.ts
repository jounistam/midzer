// Records a performance (from Play to Stop) and writes it as a MIDI file:
// your notes and every band part on their own tracks.
//
// Engine events carry the audio frame they sound at. The band's beats give
// the musical grid, so frames are mapped to ticks between the surrounding
// beats; tempo changes and drift land exactly where they were played.

import { DRUM_CHANNEL, partOfChannel, PLAYER_DRUM_CHANNEL, type TimedEvent } from '@midzer/audio-web';
import { tempoEvent, writeSmf, type TrackEvents } from './smf';

const PPQ = 480;
/** Enough for well over an hour of a busy band. */
const MAX_EVENTS = 400_000;

interface Recorded {
  frame: number;
  bytes: number[];
}

/** Each part's track; `out` is the channel in the file, when it differs. */
const TRACKS: readonly { name: string; channel: number; out?: number }[] = [
  { name: 'You', channel: 0 },
  { name: 'Your drums', channel: PLAYER_DRUM_CHANNEL, out: DRUM_CHANNEL },
  { name: 'Keys', channel: 1 },
  { name: 'Bass', channel: 2 },
  { name: 'Pad', channel: 3 },
  { name: 'Drums', channel: DRUM_CHANNEL },
];

export class Recorder {
  private beats: number[] = [];
  private events: Recorded[] = [];
  private recording = false;
  private startFrame = 0;
  private endFrame = 0;

  constructor(private readonly sampleRate: number) {}

  /** Whether a finished take is ready to export. */
  get hasTake(): boolean {
    return !this.recording && this.events.some((e) => (e.bytes[0]! & 0xf0) === 0x90);
  }

  record(event: TimedEvent): void {
    if (event.type === 'transport') {
      if (event.playing) {
        this.recording = true;
        this.beats = [];
        this.events = [];
        this.startFrame = event.frame;
      } else if (this.recording) {
        this.recording = false;
        this.endFrame = event.frame;
      }
      return;
    }
    if (!this.recording) return;
    if (event.type === 'beat') {
      this.beats.push(event.frame);
      return;
    }
    if (this.events.length >= MAX_EVENTS || partOfChannel(this.channelOf(event)) === null) return;
    const bytes =
      event.type === 'noteOn'
        ? [0x90 | event.channel, event.key, event.velocity]
        : event.type === 'noteOff'
          ? [0x80 | event.channel, event.key, 0]
          : event.type === 'programChange'
            ? [0xc0 | event.channel, event.program]
            : null;
    if (bytes) this.events.push({ frame: event.frame, bytes });
  }

  private channelOf(event: TimedEvent): number {
    return 'channel' in event ? event.channel : -1;
  }

  /** The last take as a MIDI file, or null if nothing was played. */
  export(bpm: number): Uint8Array | null {
    if (!this.hasTake) return null;
    const framesPerBeat = (60 / bpm) * this.sampleRate;
    const beats = this.beats.length > 0 ? this.beats : [this.startFrame];
    const span = (i: number) =>
      i + 1 < beats.length ? beats[i + 1]! - beats[i]! : i > 0 ? beats[i]! - beats[i - 1]! : framesPerBeat;
    const toTick = (frame: number) => {
      // Beat i sounds at beats[i]: find the last beat at or before `frame` and interpolate.
      let lo = 0;
      let hi = beats.length - 1;
      while (lo < hi) {
        const mid = (lo + hi + 1) >> 1;
        if (beats[mid]! <= frame) lo = mid;
        else hi = mid - 1;
      }
      return Math.max(0, (lo + (frame - beats[lo]!) / span(lo)) * PPQ);
    };

    const conductor: TrackEvents = { name: 'midzer', events: [[0, 0xff, 0x58, 4, 2, 24, 8]] };
    let lastBpm = 0;
    for (let i = 0; i < beats.length; i++) {
      const beatBpm = (60 * this.sampleRate) / span(i);
      if (Math.abs(beatBpm - lastBpm) > 0.05) {
        conductor.events.push(tempoEvent(i * PPQ, beatBpm));
        lastBpm = beatBpm;
      }
    }

    const endTick = toTick(Math.max(this.endFrame, this.events[this.events.length - 1]?.frame ?? 0));
    const tracks = TRACKS.map(({ name, channel, out = channel }) => {
      const sounding = new Set<number>();
      const events: number[][] = [];
      for (const e of this.events) {
        if ((e.bytes[0]! & 0x0f) !== channel) continue;
        const kind = e.bytes[0]! & 0xf0;
        if (kind === 0x90) sounding.add(e.bytes[1]!);
        else if (kind === 0x80 && !sounding.delete(e.bytes[1]!)) continue; // an orphan release
        events.push([toTick(e.frame), kind | out, ...e.bytes.slice(1)]);
      }
      for (const key of sounding) events.push([endTick, 0x80 | out, key, 0]);
      return { name, events };
    });
    return writeSmf(PPQ, [conductor, ...tracks.filter((t) => t.events.some((e) => (e[1]! & 0xf0) === 0x90))]);
  }
}
