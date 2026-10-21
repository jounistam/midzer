import { describe, expect, it } from 'vitest';
import { emptyScore } from './judge';
import { isUnlocked, lessonAfter, LESSONS, nextLesson, units } from './lessons';
import { loadProgress, recordRun, type Progress } from './progress';
import { buildTargets, partsOf } from './targets';

const done = (id: string, stars: Progress['stars']): [string, Progress] => [
  id,
  { id, stars, bestAccuracy: 1, bestStreak: 1, completed: 1, lastPlayed: 0 },
];

describe('lesson path', () => {
  it('every lesson has something to play in its part', () => {
    for (const lesson of LESSONS) {
      expect(partsOf(lesson.song), lesson.id).toContain(lesson.part);
      expect(buildTargets(lesson.song, lesson.part).length, lesson.id).toBeGreaterThan(3);
    }
    expect(new Set(LESSONS.map((l) => l.id)).size).toBe(LESSONS.length);
  });

  it('groups lessons into units in order', () => {
    expect(units().map((u) => u.unit)).toEqual(['Chords with the band', 'First melodies', 'Hands together']);
  });

  it('opens each lesson once the one before has a star', () => {
    const [first, second, third] = LESSONS;
    const none = new Map<string, Progress>();
    expect(isUnlocked(first!.id, none)).toBe(true);
    expect(isUnlocked(second!.id, none)).toBe(false);
    expect(nextLesson(none)?.id).toBe(first!.id);
    const some = new Map([done(first!.id, 1)]);
    expect(isUnlocked(second!.id, some)).toBe(true);
    expect(isUnlocked(third!.id, some)).toBe(false);
    expect(nextLesson(some)?.id).toBe(second!.id);
    expect(nextLesson(new Map([done(first!.id, 0)]))?.id).toBe(first!.id);
    expect(lessonAfter(first!.id)?.id).toBe(second!.id);
    expect(lessonAfter(LESSONS[LESSONS.length - 1]!.id)).toBeNull();
  });
});

describe('progress', () => {
  it('keeps the best of every run', async () => {
    const great = { ...emptyScore(), perfect: 9, good: 1, bestStreak: 10 };
    const poor = { ...emptyScore(), perfect: 2, miss: 8, bestStreak: 2 };
    expect((await recordRun('x', great, 1)).stars).toBe(3);
    const after = await recordRun('x', poor, 2);
    expect(after).toMatchObject({ stars: 3, bestStreak: 10, completed: 2, lastPlayed: 2 });
    expect((await loadProgress()).get('x')?.completed).toBe(2);
  });
});
