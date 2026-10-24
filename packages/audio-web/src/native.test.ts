import { describe, expect, it } from 'vitest';
import { liveSongOf } from './engine';
import { decodeInputCommand, frameTime } from './native';
import { encodeCommand } from './protocol';

describe('native host', () => {
  it('reads a captured song as complete only when heard from start to end', () => {
    expect(liveSongOf(null, 200, 3)).toEqual({ seconds: 200, complete: true });
    expect(liveSongOf(null, 200, 1).complete).toBe(false);
    expect(liveSongOf(null, 200, 2).complete).toBe(false);
    const samples = new Int16Array(4);
    expect(liveSongOf(samples, 1, 3).samples).toBe(samples);
  });

  it('times frames against the frame being heard', () => {
    expect(frameTime(48_000 + 480, 48_000, 48_000, 1000)).toBeCloseTo(1010);
    expect(frameTime(47_520, 48_000, 48_000, 1000)).toBeCloseTo(990);
    // u32 frames wrap; the nearest unwrapped frame wins.
    expect(frameTime(100, 2 ** 32 - 380, 48_000, 0)).toBeCloseTo(10);
  });

  it('decodes MIDI input commands as command.rs encodes them', () => {
    for (const cmd of [
      { type: 'noteOn', note: 60, velocity: 99 },
      { type: 'noteOff', note: 61 },
      { type: 'allNotesOff' },
      { type: 'drumNote', note: 38, velocity: 120 },
      { type: 'hiHatPedal', value: 90 },
      { type: 'drumChoke', note: 49 },
      { type: 'controlChange', cc: 64, value: 127 },
    ] as const) {
      const [op, a, b] = encodeCommand(cmd);
      expect(decodeInputCommand(op, a, b)).toEqual(cmd);
    }
    expect(decodeInputCommand(5, 0, 0)).toBeNull();
  });
});
