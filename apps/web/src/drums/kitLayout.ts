// Where each piece sits in the top-down kit view. Positions are on a 12 × 5
// grid, scaled to fit and centred; a right-handed kit seen from the throne.

import type { DrumPiece } from '@midzer/audio-web';

export type PadShape = 'cymbal' | 'drum' | 'ring' | 'pedal';

export interface Pad {
  piece: DrumPiece;
  shape: PadShape;
  x: number;
  y: number;
  r: number;
}

const GRID_W = 12;
const GRID_H = 5;

/** Rings (rims, open hi-hat, ride bow) sit around a smaller pad on the same spot. */
const GRID: readonly { piece: DrumPiece; shape: PadShape; gx: number; gy: number; gr: number }[] = [
  { piece: 'hiHatOpen', shape: 'ring', gx: 1.3, gy: 2.1, gr: 1.0 },
  { piece: 'hiHatClosed', shape: 'cymbal', gx: 1.3, gy: 2.1, gr: 0.6 },
  { piece: 'hiHatPedal', shape: 'pedal', gx: 1.3, gy: 4.25, gr: 0.45 },
  { piece: 'crash', shape: 'cymbal', gx: 3.1, gy: 0.85, gr: 0.8 },
  { piece: 'splash', shape: 'cymbal', gx: 5.9, gy: 0.5, gr: 0.45 },
  { piece: 'snareRim', shape: 'ring', gx: 3.5, gy: 3.15, gr: 0.95 },
  { piece: 'snare', shape: 'drum', gx: 3.5, gy: 3.15, gr: 0.68 },
  { piece: 'sideStick', shape: 'pedal', gx: 2.7, gy: 4.45, gr: 0.32 },
  { piece: 'tom1', shape: 'drum', gx: 4.9, gy: 1.75, gr: 0.6 },
  { piece: 'tom2', shape: 'drum', gx: 6.9, gy: 1.75, gr: 0.6 },
  { piece: 'kick', shape: 'drum', gx: 5.9, gy: 3.65, gr: 0.95 },
  { piece: 'crash2', shape: 'cymbal', gx: 8.6, gy: 0.75, gr: 0.75 },
  { piece: 'tom3', shape: 'drum', gx: 8.3, gy: 3.3, gr: 0.8 },
  { piece: 'tom4', shape: 'drum', gx: 9.55, gy: 4.25, gr: 0.55 },
  { piece: 'ride', shape: 'ring', gx: 10.4, gy: 2.2, gr: 1.1 },
  { piece: 'rideBell', shape: 'cymbal', gx: 10.4, gy: 2.2, gr: 0.38 },
  { piece: 'china', shape: 'cymbal', gx: 11.1, gy: 0.55, gr: 0.5 },
];

export function layoutKit(width: number, height: number): Pad[] {
  const scale = Math.min(width / GRID_W, height / GRID_H);
  const ox = (width - GRID_W * scale) / 2;
  const oy = (height - GRID_H * scale) / 2;
  return GRID.map(({ piece, shape, gx, gy, gr }) => ({ piece, shape, x: ox + gx * scale, y: oy + gy * scale, r: gr * scale }));
}

/** The piece under a point: the smallest pad containing it, so a bell wins over its ride. */
export function pieceAt(pads: readonly Pad[], x: number, y: number): DrumPiece | null {
  let best: Pad | null = null;
  for (const pad of pads) {
    if ((x - pad.x) ** 2 + (y - pad.y) ** 2 <= pad.r ** 2 && (!best || pad.r < best.r)) best = pad;
  }
  return best?.piece ?? null;
}
