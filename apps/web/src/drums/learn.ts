// Learning an e-kit's notes: the player hits each piece in turn, and every
// note heard during that piece's step plays it. Kits send several notes for
// one piece (head and rim, bow and edge), so a step collects all of them.

import type { DrumPiece } from '@midzer/audio-web';
import { PIECES, type KitMap } from './kit';

export interface KitLearning {
  /** Index into PIECES; PIECES.length when every piece has had its turn. */
  step: number;
  map: KitMap;
}

export function startLearning(): KitLearning {
  return { step: 0, map: {} };
}

export function currentPiece(l: KitLearning): DrumPiece | null {
  return PIECES[l.step] ?? null;
}

/** A note heard while learning plays the current piece, even if it played another before. */
export function hear(l: KitLearning, note: number): KitLearning {
  const piece = currentPiece(l);
  if (!piece || l.map[note] === piece) return l;
  return { ...l, map: { ...l.map, [note]: piece } };
}

export function notesOf(map: KitMap, piece: DrumPiece): number[] {
  return Object.entries(map)
    .filter(([, p]) => p === piece)
    .map(([note]) => Number(note))
    .sort((a, b) => a - b);
}

/** Forgets the current piece's notes, to hit it again. */
export function clearPiece(l: KitLearning): KitLearning {
  const piece = currentPiece(l);
  return { ...l, map: Object.fromEntries(Object.entries(l.map).filter(([, p]) => p !== piece)) };
}

export function move(l: KitLearning, by: 1 | -1): KitLearning {
  return { ...l, step: Math.min(PIECES.length, Math.max(0, l.step + by)) };
}

export function isDone(l: KitLearning): boolean {
  return l.step >= PIECES.length;
}
