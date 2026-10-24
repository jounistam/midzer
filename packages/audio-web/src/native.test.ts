import { describe, expect, it } from 'vitest';
import { liveFollowOf, liveSongOf } from './engine';
import { decodeInputCommand, encodeLibrary, frameTime } from './native';
import { encodeCommand } from './protocol';

describe('native host', () => {
  it('reads a captured song as complete only when heard from start to end', () => {
    expect(liveSongOf(null, 200, 3)).toEqual({ seconds: 200, complete: true });
    expect(liveSongOf(null, 200, 1).complete).toBe(false);
    expect(liveSongOf(null, 200, 2).complete).toBe(false);
    const samples = new Int16Array(4);
    expect(liveSongOf(samples, 1, 3).samples).toBe(samples);
    expect(liveSongOf(null, 200, 3, 4).followed).toBe(4);
    expect(liveSongOf(null, 200, 3, -1)).not.toHaveProperty('followed');
  });

  it('reads a follow, and −1 as none', () => {
    expect(liveFollowOf(2, 31.5, 1000)).toEqual({ id: 2, seconds: 31.5, timeMs: 1000 });
    expect(liveFollowOf(-1, 0, 0)).toBeNull();
  });

  it('lays the library out as live_library reads it', () => {
    const bytes = encodeLibrary([
      { id: 7, landmarks: new Uint32Array([1, 2, 3, 4]), beatTimes: new Float64Array([0.5, 1]), chords: new Int32Array([-1]), seconds: 30, barBeats: new Uint8Array([4, 2, 3]) },
    ]);
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    expect(bytes.byteLength).toBe(4 + 32 + 16 + 16 + 4 + 4);
    expect([0, 4, 8, 12, 16, 20].map((at) => view.getUint32(at, true))).toEqual([1, 7, 2, 2, 1, 3]);
    expect(view.getFloat64(28, true)).toBe(30);
    expect(view.getUint32(36 + 12, true)).toBe(4);
    expect(view.getFloat64(52, true)).toBe(0.5);
    expect(view.getInt32(68, true)).toBe(-1);
    expect([...bytes.subarray(72, 75)]).toEqual([4, 2, 3]);
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
