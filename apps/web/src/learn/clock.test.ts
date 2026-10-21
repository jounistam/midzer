import { describe, expect, it } from 'vitest';
import { SongClock } from './clock';

describe('song clock', () => {
  it('moves on between beats, takes anchors at their time and never runs past the next beat', () => {
    const clock = new SongClock();
    clock.setTempo(120); // 500 ms a beat
    clock.onBeat(0, 1000);
    clock.onBeat(1, 1500);
    expect(clock.position(900)).toBeNull();
    expect(clock.position(1250)).toBeCloseTo(0.5);
    expect(clock.position(1600)).toBeCloseTo(1.2);
    expect(clock.position(5000)).toBe(2);
  });

  it('stands still while the band waits and goes on from there', () => {
    const clock = new SongClock();
    clock.setTempo(120);
    clock.onBeat(4, 0);
    clock.onHold(true, 4.5, 250);
    clock.onHold(false, 4.5, 3000);
    expect(clock.position(2000)).toBe(4.5);
    expect(clock.holding(2000)).toBe(true);
    expect(clock.position(3100)).toBeCloseTo(4.7);
    expect(clock.holding(3100)).toBe(false);
  });

  it('follows a loop back to its start', () => {
    const clock = new SongClock();
    clock.setTempo(60);
    clock.onBeat(7, 0);
    clock.onBeat(4, 1000);
    expect(clock.position(1500)).toBeCloseTo(4.5);
  });
});
