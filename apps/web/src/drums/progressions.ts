// Progression loops: the harmony a drummer plays over in Jam, since drums
// form no chords. Each is a short chart in song notation that loops through
// song mode. Chord progressions are free to use.

import type { EngineCommand } from '@midzer/audio-web';
import { chartCommands, parseChart, type Bar } from '../songs/song';

export interface Progression {
  id: string;
  name: string;
  /** Roman numerals or a short description. */
  hint: string;
  chart: string;
}

/** A few progressions per style, in `STYLES` order; the first is the default. */
export const PROGRESSIONS: readonly (readonly Progression[])[] = [
  // Pop
  [
    { id: 'pop-anthem', name: 'Four-chord anthem', hint: 'I–V–vi–IV in C', chart: 'C | G | Am | F' },
    { id: 'pop-sad', name: 'Minor turn', hint: 'vi–IV–I–V in C', chart: 'Am | F | C | G' },
    { id: 'pop-doo-wop', name: 'Doo-wop', hint: 'I–vi–IV–V in G', chart: 'G | Em | C | D' },
  ],
  // Rock
  [
    { id: 'rock-mixo', name: 'Mixolydian', hint: 'I–♭VII–IV–I in D', chart: 'D | C | G | D' },
    { id: 'rock-three', name: 'Three chords', hint: 'I–IV–V–IV in A', chart: 'A | D | E | D' },
    { id: 'rock-minor', name: 'Minor drive', hint: 'i–♭VI–♭III–♭VII in E', chart: 'Em | C | G | D' },
  ],
  // Funk
  [
    { id: 'funk-vamp', name: 'One-chord vamp', hint: 'E7 all the way', chart: 'E7 | E7 | E7 | E7' },
    { id: 'funk-two', name: 'Two-chord groove', hint: 'i7–IV7 in E', chart: 'Em7 | A7 | Em7 | A7' },
    { id: 'funk-ii-v', name: 'ii–V vamp', hint: 'ii7–V7 in C', chart: 'Dm7 | G7 | Dm7 | G7' },
  ],
  // Bossa
  [
    { id: 'bossa-251', name: 'ii–V–I', hint: 'in C major', chart: 'Dm7 | G7 | Cmaj7 | Cmaj7' },
    { id: 'bossa-minor', name: 'Minor sway', hint: 'i–iv–V in A minor', chart: 'Am7 | Dm7 | Am7 | E7' },
    { id: 'bossa-turn', name: 'Turnaround', hint: 'I–vi–ii–V in F', chart: 'Fmaj7 | Dm7 | Gm7 | C7' },
  ],
  // Ballad
  [
    { id: 'ballad-canon', name: 'Canon', hint: 'After Pachelbel, in D', chart: 'D A | Bm F#m | G D | G A' },
    { id: 'ballad-rise', name: 'Slow rise', hint: 'I–iii–IV–V in C', chart: 'C | Em | F | G' },
    { id: 'ballad-eight', name: 'Eight-bar ballad', hint: 'I–V–vi–iii–IV–I–IV–V in C', chart: 'C | G | Am | Em | F | C | F | G' },
  ],
  // Blues
  [
    {
      id: 'blues-twelve',
      name: 'Twelve-bar blues',
      hint: 'in A',
      chart: 'A7 | A7 | A7 | A7 | D7 | D7 | A7 | A7 | E7 | D7 | A7 | E7',
    },
    {
      id: 'blues-quick',
      name: 'Quick change',
      hint: 'IV in bar 2, in E',
      chart: 'E7 | A7 | E7 | E7 | A7 | A7 | E7 | E7 | B7 | A7 | E7 | B7',
    },
    {
      id: 'blues-minor',
      name: 'Minor blues',
      hint: 'in A minor',
      chart: 'Am7 | Am7 | Am7 | Am7 | Dm7 | Dm7 | Am7 | Am7 | F7 | E7 | Am7 | E7',
    },
  ],
];

export function progressionsFor(style: number): readonly Progression[] {
  return PROGRESSIONS[style] ?? PROGRESSIONS[0]!;
}

/** The progression `id` if the style has it, else the style's first. */
export function pickProgression(style: number, id: string | null): Progression {
  const list = progressionsFor(style);
  return list.find((p) => p.id === id) ?? list[0]!;
}

export function progressionBars(progression: Progression): Bar[] {
  return parseChart(progression.chart).bars;
}

/** Loads a progression into the engine's song chart, looping all of it. */
export function progressionCommands(progression: Progression): EngineCommand[] {
  const bars = progressionBars(progression);
  return [...chartCommands({ bars }), { type: 'songLoop', start: 0, end: bars.length }];
}
