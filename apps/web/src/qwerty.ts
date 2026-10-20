// Computer-keyboard piano layout: the home row plays white keys and the row
// above plays black keys, starting at middle C.
const KEY_TO_OFFSET: Record<string, number> = {
  a: 0, w: 1, s: 2, e: 3, d: 4, f: 5, t: 6, g: 7, y: 8, h: 9, u: 10, j: 11, k: 12,
};

export const BASE_NOTE = 60;
export const KEY_COUNT = 13;

export function qwertyToNote(key: string): number | undefined {
  const offset = KEY_TO_OFFSET[key.toLowerCase()];
  return offset === undefined ? undefined : BASE_NOTE + offset;
}

export function noteToQwerty(note: number): string | undefined {
  return Object.keys(KEY_TO_OFFSET).find((k) => KEY_TO_OFFSET[k] === note - BASE_NOTE);
}
