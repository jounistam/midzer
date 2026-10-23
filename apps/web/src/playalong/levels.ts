// What a keys player is asked to play for each chord heard, by level: from one
// key with the right hand up to full chords with both hands. The voicings
// depend on the chord alone, so a chord shows the same keys every time.

import { NOTE_NAMES, QUALITY_INTERVALS, QUALITY_SUFFIXES } from '@midzer/audio-web';

export type AlongLevel = 1 | 2 | 3 | 4 | 5;

export const ALONG_LEVELS: { level: AlongLevel; name: string; detail: string }[] = [
  { level: 1, name: 'One key', detail: "The chord's root with the right hand, one key at a time." },
  { level: 2, name: 'Three keys', detail: 'The whole chord with the right hand, root at the bottom.' },
  { level: 3, name: 'Close moves', detail: 'The chord turned around so the right hand hardly moves between chords.' },
  { level: 4, name: 'Both hands', detail: 'The root low with the left hand, the chord with the right.' },
  { level: 5, name: 'Full chords', detail: 'Root and fifth with the left hand, every chord tone with the right, sevenths too.' },
];

export const DEFAULT_LEVEL: AlongLevel = 1;

export interface ChordShape {
  root: number;
  quality: number;
}

/** Keys for each hand, low to high. */
export interface Voicing {
  left: number[];
  right: number[];
}

/** Middle C: the right hand's root at levels 1 and 2. */
const RIGHT_C = 60;
/** The right hand's close voicings sit as near this key as they can. */
const RIGHT_CENTRE = 65;
/** The left hand's root, C2..B2. */
const LEFT_C = 36;

const quality = (suffix: string) => QUALITY_SUFFIXES.indexOf(suffix as (typeof QUALITY_SUFFIXES)[number]);
/** Sixths, sevenths and added notes left out, below level 5. */
const TRIAD: Record<string, string> = { '6': '', '7': '', maj7: '', add9: '', m6: 'm', m7: 'm', m7b5: 'dim', dim7: 'dim' };

function triadOf(q: number): number {
  const simple = TRIAD[QUALITY_SUFFIXES[q] ?? ''];
  return simple === undefined ? q : quality(simple);
}

/** The close voicing of `intervals` over `root` whose keys average nearest `centre`, root position on a tie. */
function closeNear(root: number, intervals: readonly number[], centre: number): number[] {
  let best: number[] = [];
  let bestOff = Infinity;
  for (let r = 0; r < intervals.length; r++) {
    const pcs = [...intervals.slice(r), ...intervals.slice(0, r)].map((iv) => (root + iv) % 12);
    const notes = [pcs[0]!];
    for (const pc of pcs.slice(1)) notes.push(notes[notes.length - 1]! + ((pc - notes[notes.length - 1]! + 12) % 12 || 12));
    const mean = notes.reduce((a, b) => a + b, 0) / notes.length;
    const shift = Math.round((centre - mean) / 12) * 12;
    const off = Math.abs(mean + shift - centre);
    if (off < bestOff - 1e-9) {
      bestOff = off;
      best = notes.map((n) => n + shift);
    }
  }
  return best;
}

export function voicing(chord: ChordShape, level: AlongLevel): Voicing {
  const root = chord.root % 12;
  const triad = QUALITY_INTERVALS[triadOf(chord.quality)] ?? [0];
  switch (level) {
    case 1:
      return { left: [], right: [RIGHT_C + root] };
    case 2:
      return { left: [], right: triad.map((iv) => RIGHT_C + root + iv) };
    case 3:
      return { left: [], right: closeNear(root, triad, RIGHT_CENTRE) };
    case 4:
      return { left: [LEFT_C + root], right: closeNear(root, triad, RIGHT_CENTRE) };
    case 5:
      return { left: [LEFT_C + root, LEFT_C + root + 7], right: closeNear(root, QUALITY_INTERVALS[chord.quality] ?? triad, RIGHT_CENTRE) };
  }
}

/** "C E G": the keys' note names, low to high. */
export function keyNames(keys: readonly number[]): string {
  return keys.map((k) => NOTE_NAMES[k % 12]).join(' ');
}

export function parseLevel(value: unknown): AlongLevel {
  return ALONG_LEVELS.some((l) => l.level === value) ? (value as AlongLevel) : DEFAULT_LEVEL;
}
