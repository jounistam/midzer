import { describe, expect, it } from 'vitest';
import { MAX_REPLAY_NOTES, ownShare, replayCommands, TakeRecorder, TICKS_PER_BAR } from './replay';

describe('recording a solo', () => {
  it('places the notes from the first bar, as played', () => {
    const r = new TakeRecorder();
    expect(r.take()).toBeNull();
    r.note({ key: 60, velocity: 90, bar: 7, tick: 480, own: false });
    r.note({ key: 60, velocity: 0, bar: 7, tick: 720, own: false });
    r.note({ key: 64, velocity: 100, bar: 8, tick: 0, own: true });
    r.note({ key: 64, velocity: 0, bar: 8, tick: 960, own: true });
    expect(r.take()).toEqual({
      notes: [
        { tick: 480, len: 240, key: 60, velocity: 90, own: false },
        { tick: TICKS_PER_BAR, len: 960, key: 64, velocity: 100, own: true },
      ],
      bars: 2,
    });
  });

  it('ends notes still sounding, and a retriggered key ends the one before', () => {
    const r = new TakeRecorder();
    r.note({ key: 62, velocity: 80, bar: 0, tick: 0, own: false });
    r.note({ key: 62, velocity: 70, bar: 0, tick: 240, own: false });
    r.note({ key: 67, velocity: 70, bar: 0, tick: 480, own: true });
    r.note({ key: 69, velocity: 0, bar: 0, tick: 600, own: true });
    const take = r.take()!;
    expect(take.notes.map((n) => [n.tick, n.len])).toEqual([
      [0, 240],
      [240, 360],
      [480, 120],
    ]);
  });

  it('copes with bars that wrap and notes heard just before the first bar', () => {
    const r = new TakeRecorder();
    r.note({ key: 60, velocity: 90, bar: 65_535, tick: 1800, own: false });
    r.note({ key: 62, velocity: 90, bar: 65_535, tick: 1700, own: true });
    r.note({ key: 64, velocity: 90, bar: 0, tick: 0, own: false });
    const take = r.take()!;
    expect(take.notes.map((n) => [n.tick, n.key])).toEqual([
      [1700, 62],
      [1800, 60],
      [TICKS_PER_BAR, 64],
    ]);
    const before = new TakeRecorder();
    before.note({ key: 60, velocity: 90, bar: 3, tick: 0, own: false });
    before.note({ key: 62, velocity: 90, bar: 2, tick: 1800, own: true });
    expect(before.take()!.notes.map((n) => n.tick)).toEqual([1800, TICKS_PER_BAR]);
  });

  it('loads into the engine, up to its size', () => {
    const r = new TakeRecorder();
    for (let i = 0; i < MAX_REPLAY_NOTES + 5; i++) r.note({ key: 60 + (i % 5), velocity: 80, bar: 0, tick: i * 60, own: i % 4 === 0 });
    const take = r.take()!;
    const commands = replayCommands(take);
    expect(commands[0]).toEqual({ type: 'replayClear' });
    expect(commands).toHaveLength(MAX_REPLAY_NOTES + 1);
    expect(commands[1]).toEqual({ type: 'replayNote', tick: 0, len: 300, key: 60, velocity: 80 });
    expect(ownShare(take)).toBeCloseTo(0.25, 2);
  });
});
