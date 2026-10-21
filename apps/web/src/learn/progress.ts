// Learn progress, per lesson or per practised song part, kept in IndexedDB.

import { store } from '../storage/db';
import { accuracy, starsFor, type Score } from './judge';

export interface Progress {
  /** A lesson id, or `song:<song id>:<part>`. */
  id: string;
  /** Best result of a whole run. */
  stars: 0 | 1 | 2 | 3;
  bestAccuracy: number;
  bestStreak: number;
  /** Whole runs played to the end. */
  completed: number;
  lastPlayed: number;
}

const progress = store<Progress>('progress');

export async function loadProgress(): Promise<Map<string, Progress>> {
  try {
    return new Map((await progress.all()).map((p) => [p.id, p]));
  } catch {
    return new Map();
  }
}

/** Records a run played to the end, keeping the bests. Returns the updated progress. */
export async function recordRun(id: string, score: Score, now = Date.now()): Promise<Progress> {
  const before = await progress.get(id).catch(() => undefined);
  const stars = starsFor(score);
  const next: Progress = {
    id,
    stars: Math.max(before?.stars ?? 0, stars) as Progress['stars'],
    bestAccuracy: Math.max(before?.bestAccuracy ?? 0, accuracy(score)),
    bestStreak: Math.max(before?.bestStreak ?? 0, score.bestStreak),
    completed: (before?.completed ?? 0) + 1,
    lastPlayed: now,
  };
  await progress.put(id, next).catch(() => undefined);
  return next;
}

export const songProgressId = (songId: string, part: string) => `song:${songId}:${part}`;
