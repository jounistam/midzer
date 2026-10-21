import { describe, expect, it } from 'vitest';
import { accuracy, Judge, starsFor } from './judge';
import type { Target } from './targets';

const MS = 500; // a beat at 120 BPM
const note = (id: number, beat: number, key: number): Target => ({ id, kind: 'note', beat, len: 1, keys: [key], pcs: [key % 12] });
const chord = (id: number, beat: number, name: string, keys: number[]): Target => ({
  id,
  kind: 'chord',
  beat,
  len: 4,
  keys,
  pcs: keys.map((k) => k % 12),
  name,
});

describe('judge, playing in time', () => {
  it('grades notes by how close to the beat they are', () => {
    const judge = new Judge([note(0, 0, 60), note(1, 1, 62), note(2, 2, 64), note(3, 3, 65)], { wait: false, anyOctave: false });
    expect(judge.noteOn(60, 0.1, MS)[0]).toMatchObject({ grade: 'perfect', offsetMs: 50 });
    expect(judge.noteOn(62, 0.75, MS)[0]).toMatchObject({ grade: 'good', offsetMs: -125 });
    expect(judge.noteOn(64, 2.4, MS)[0]).toMatchObject({ grade: 'ok' });
    expect(judge.advance(3.4, MS)).toEqual([]);
    expect(judge.advance(3.6, MS).map((j) => j.grade)).toEqual(['miss']);
    expect(judge.score).toMatchObject({ perfect: 1, good: 1, ok: 1, miss: 1, streak: 0, bestStreak: 3 });
    expect(judge.done()).toBe(true);
    expect(accuracy(judge.score)).toBeCloseTo(225 / 400);
    expect(starsFor(judge.score)).toBe(1);
  });

  it('counts wrong notes but not notes of a chord on its way', () => {
    const judge = new Judge([chord(0, 4, 'C', [48, 52, 55])], { wait: false, anyOctave: false });
    expect(judge.noteOn(48, 3.9, MS)).toEqual([]);
    expect(judge.noteOn(64, 3.95, MS)).toEqual([]); // E in another octave: still part of C
    expect(judge.noteOn(61, 4.0, MS)).toEqual([]); // C#: wrong
    const [hit] = judge.noteOn(55, 4.05, MS);
    expect(hit).toMatchObject({ grade: 'perfect' });
    expect(judge.score.wrong).toBe(1);
    expect(judge.noteOn(60, 4.1, MS)).toEqual([]); // doubling the chord just played is fine
    expect(judge.score.wrong).toBe(1);
  });

  it('a chord held over the bar line must be struck again', () => {
    const keys = [60, 64, 67];
    const judge = new Judge([chord(0, 0, 'C', keys), chord(1, 4, 'C', keys)], { wait: false, anyOctave: false });
    for (const k of keys) judge.noteOn(k, 0, MS);
    expect(judge.advance(4.6, MS).map((j) => j.target.id)).toEqual([1]);
  });

  it('can take melody notes in any octave', () => {
    const judge = new Judge([note(0, 0, 76)], { wait: false, anyOctave: true });
    expect(judge.noteOn(64, 0, MS)[0]?.grade).toBe('perfect');
  });

  it('starts a loop pass over and skips what a jump goes past', () => {
    const judge = new Judge([note(0, 0, 60), note(1, 1, 62), note(2, 8, 64)], { wait: false, anyOctave: false });
    judge.noteOn(60, 0, MS);
    judge.noteOn(62, 1, MS);
    judge.advance(1.8, MS);
    judge.advance(0, MS);
    expect(judge.states).toEqual(['open', 'open', 'open']);
    judge.advance(0.2, MS);
    judge.advance(6, MS);
    expect(judge.states).toEqual(['miss', 'skipped', 'open']);
  });
});

describe('judge, wait mode', () => {
  it('waits for the right notes: clean is perfect, after a wrong note good', () => {
    const judge = new Judge([note(0, 2, 60), note(1, 3, 62)], { wait: true, anyOctave: false });
    expect(judge.noteOn(60, 0, MS)).toEqual([]); // far too early: neither right nor wrong
    expect(judge.score.wrong).toBe(0);
    expect(judge.noteOn(60, 1.6, MS)[0]?.grade).toBe('perfect');
    expect(judge.nextOpen()?.id).toBe(1);
    expect(judge.noteOn(61, 3, MS)).toEqual([]);
    expect(judge.noteOn(62, 3, MS)[0]?.grade).toBe('good');
    expect(judge.score).toMatchObject({ perfect: 1, good: 1, wrong: 1 });
    expect(judge.advance(10, MS)).toEqual([]);
  });

  it('a chord and a melody note on the same beat both have to be played', () => {
    const judge = new Judge([chord(0, 0, 'F', [53, 57, 60]), note(1, 0, 69)], { wait: true, anyOctave: false });
    expect(judge.current().map((t) => t.id)).toEqual([0, 1]);
    judge.noteOn(69, 0, MS);
    expect(judge.current().map((t) => t.id)).toEqual([0]);
    for (const k of [53, 57]) judge.noteOn(k, 0, MS);
    expect(judge.noteOn(60, 0, MS).map((j) => j.target.id)).toEqual([0]);
    expect(judge.done()).toBe(true);
    expect(judge.score.wrong).toBe(0);
  });
});
