import { describe, expect, it } from 'vitest';
import { PIECES } from './kit';
import { clearPiece, currentPiece, hear, isDone, move, notesOf, startLearning } from './learn';

describe('learning a kit', () => {
  it('collects every note heard for a piece, and moves notes between pieces', () => {
    let l = startLearning();
    expect(currentPiece(l)).toBe('kick');
    l = hear(hear(l, 36), 36);
    l = move(l, 1);
    expect(currentPiece(l)).toBe('snare');
    l = hear(hear(l, 38), 40);
    expect(notesOf(l.map, 'snare')).toEqual([38, 40]);
    // 40 is really the rim: hitting it in the rim's step moves it there.
    l = hear(move(l, 1), 40);
    expect(notesOf(l.map, 'snare')).toEqual([38]);
    expect(notesOf(l.map, 'snareRim')).toEqual([40]);
    expect(notesOf(l.map, 'kick')).toEqual([36]);
  });

  it('clears a piece, and ends after the last one', () => {
    let l = hear(startLearning(), 36);
    l = clearPiece(l);
    expect(l.map).toEqual({});
    for (let i = 0; i < PIECES.length + 3; i++) l = move(l, 1);
    expect(isDone(l)).toBe(true);
    expect(currentPiece(l)).toBeNull();
    expect(hear(l, 50)).toBe(l);
    expect(move(startLearning(), -1).step).toBe(0);
  });
});
