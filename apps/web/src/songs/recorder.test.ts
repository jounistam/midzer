import type { TimedEvent } from '@midzer/audio-web';
import { describe, expect, it } from 'vitest';
import { Recorder } from './recorder';
import { readSmf } from './smf';

const SR = 48_000;

describe('Recorder', () => {
  it('exports a take on the beat grid, one track per part', () => {
    const rec = new Recorder(SR);
    const events: TimedEvent[] = [
      { type: 'transport', playing: true, frame: 1000 },
      { type: 'programChange', channel: 2, program: 33, frame: 1000 },
      // 120 bpm: a beat every 24000 frames.
      ...[0, 1, 2, 3, 4].map((i) => ({ type: 'beat' as const, bar: 0, beat: i % 4, frame: 1000 + i * 24_000 })),
      { type: 'noteOn', channel: 0, key: 60, velocity: 90, frame: 1000 + 12_000 },
      { type: 'noteOff', channel: 0, key: 60, frame: 1000 + 36_000 },
      { type: 'noteOn', channel: 2, key: 36, velocity: 80, frame: 1000 },
      { type: 'noteOn', channel: 9, key: 42, velocity: 70, frame: 1000 + 24_000 },
      { type: 'noteOff', channel: 9, key: 42, frame: 1000 + 30_000 },
      { type: 'chord', chord: null, frame: 1000 },
      { type: 'transport', playing: false, frame: 1000 + 96_000 },
    ];
    expect(rec.hasTake).toBe(false);
    for (const e of events) rec.record(e);
    expect(rec.hasTake).toBe(true);

    const smf = readSmf(rec.export(120)!);
    expect(smf.ppq).toBe(480);
    expect(smf.tempos).toEqual([{ tick: 0, usPerQuarter: 500_000 }]);
    expect(smf.tracks.map((t) => t.name)).toEqual(['midzer', 'You', 'Bass', 'Drums']);
    const [, you, bass, drums] = smf.tracks;
    expect(you!.notes).toEqual([{ tick: 240, duration: 480, channel: 0, key: 60, velocity: 90 }]);
    expect(bass!.programs).toEqual([{ tick: 0, channel: 2, program: 33 }]);
    expect(bass!.notes[0]!.duration).toBe(1920); // held to the end of the take
    expect(drums!.notes[0]!.tick).toBe(480);
  });

  it('writes tempo changes where the beats speed up', () => {
    const rec = new Recorder(SR);
    rec.record({ type: 'transport', playing: true, frame: 0 });
    const frames = [0, 24_000, 48_000, 68_000, 88_000];
    frames.forEach((frame, i) => rec.record({ type: 'beat', bar: 0, beat: i % 4, frame }));
    rec.record({ type: 'noteOn', channel: 0, key: 60, velocity: 90, frame: 78_000 });
    rec.record({ type: 'transport', playing: false, frame: 90_000 });
    const smf = readSmf(rec.export(120)!);
    expect(smf.tempos.map((t) => [t.tick, Math.round(60_000_000 / t.usPerQuarter)])).toEqual([
      [0, 120],
      [960, 144],
    ]);
    expect(smf.tracks[1]!.notes[0]!.tick).toBe(1680); // halfway between beats 3 and 4
  });

  it('starts a new take on every Play', () => {
    const rec = new Recorder(SR);
    rec.record({ type: 'transport', playing: true, frame: 0 });
    rec.record({ type: 'noteOn', channel: 0, key: 60, velocity: 90, frame: 10 });
    rec.record({ type: 'transport', playing: false, frame: 100 });
    rec.record({ type: 'transport', playing: true, frame: 200 });
    rec.record({ type: 'transport', playing: false, frame: 300 });
    expect(rec.hasTake).toBe(false);
  });
});
