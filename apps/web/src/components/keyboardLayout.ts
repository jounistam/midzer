// Piano key geometry, independent of the renderer so it can be unit tested.

export interface KeyRect {
  note: number;
  black: boolean;
  x: number;
  y: number;
  w: number;
  h: number;
}

const BLACK_PCS = new Set([1, 3, 6, 8, 10]);
const BLACK_WIDTH = 0.62; // of a white key
const BLACK_HEIGHT = 0.62; // of the keyboard

export const isBlack = (note: number) => BLACK_PCS.has(note % 12);

/** Lays out keys `low..=high` (both should be white keys) to fill `width × height`. */
export function layoutKeys(low: number, high: number, width: number, height: number): KeyRect[] {
  const whites = [];
  for (let n = low; n <= high; n++) if (!isBlack(n)) whites.push(n);
  const ww = width / whites.length;
  const keys: KeyRect[] = whites.map((note, i) => ({ note, black: false, x: i * ww, y: 0, w: ww, h: height }));
  for (let n = low + 1; n < high; n++) {
    if (!isBlack(n)) continue;
    const leftWhite = whites.indexOf(n - 1);
    const bw = ww * BLACK_WIDTH;
    keys.push({ note: n, black: true, x: (leftWhite + 1) * ww - bw / 2, y: 0, w: bw, h: height * BLACK_HEIGHT });
  }
  return keys;
}

/** The key under a point; black keys win because they sit on top. */
export function keyAt(keys: readonly KeyRect[], x: number, y: number): number | null {
  let hit: KeyRect | null = null;
  for (const k of keys) {
    if (x >= k.x && x < k.x + k.w && y >= k.y && y < k.y + k.h && (!hit || k.black)) hit = k;
  }
  return hit?.note ?? null;
}

/** x position of the boundary just below `note`, for drawing the split marker. */
export function boundaryX(keys: readonly KeyRect[], note: number): number | null {
  const white = keys.find((k) => !k.black && k.note === note);
  return white ? white.x : null;
}
