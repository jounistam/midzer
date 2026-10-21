import { describe, expect, it } from 'vitest';
import { decodeInputCommand, frameTime } from './native';
import { encodeCommand } from './protocol';

describe('native host', () => {
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
    ] as const) {
      const [op, a, b] = encodeCommand(cmd);
      expect(decodeInputCommand(op, a, b)).toEqual(cmd);
    }
    expect(decodeInputCommand(5, 0, 0)).toBeNull();
  });
});
