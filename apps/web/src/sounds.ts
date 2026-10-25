// The sounds the player's keys can play: General MIDI programs, all of which
// GeneralUser GS has, picked and grouped for playing over the band.

export interface Sound {
  program: number;
  name: string;
}

export const SOUND_GROUPS: { name: string; sounds: Sound[] }[] = [
  {
    name: 'Keys',
    sounds: [
      { program: 0, name: 'Grand piano' },
      { program: 1, name: 'Bright piano' },
      { program: 4, name: 'Electric piano' },
      { program: 5, name: 'FM piano' },
      { program: 7, name: 'Clavinet' },
      { program: 6, name: 'Harpsichord' },
    ],
  },
  {
    name: 'Organ',
    sounds: [
      { program: 16, name: 'Drawbar organ' },
      { program: 18, name: 'Rock organ' },
      { program: 19, name: 'Church organ' },
    ],
  },
  {
    name: 'Strings and choir',
    sounds: [
      { program: 48, name: 'Strings' },
      { program: 49, name: 'Slow strings' },
      { program: 50, name: 'Synth strings' },
      { program: 45, name: 'Pizzicato' },
      { program: 52, name: 'Choir' },
    ],
  },
  {
    name: 'Pads',
    sounds: [
      { program: 89, name: 'Warm pad' },
      { program: 88, name: 'New age pad' },
      { program: 90, name: 'Polysynth' },
      { program: 91, name: 'Space voice' },
      { program: 92, name: 'Bowed glass' },
      { program: 94, name: 'Halo pad' },
      { program: 95, name: 'Sweep pad' },
    ],
  },
  {
    name: 'Synths',
    sounds: [
      { program: 81, name: 'Saw lead' },
      { program: 80, name: 'Square lead' },
      { program: 87, name: 'Bass and lead' },
      { program: 62, name: 'Synth brass' },
      { program: 38, name: 'Synth bass' },
    ],
  },
  {
    name: 'More',
    sounds: [
      { program: 11, name: 'Vibraphone' },
      { program: 12, name: 'Marimba' },
      { program: 10, name: 'Music box' },
      { program: 24, name: 'Nylon guitar' },
      { program: 27, name: 'Clean guitar' },
      { program: 61, name: 'Brass' },
      { program: 65, name: 'Alto sax' },
      { program: 73, name: 'Flute' },
    ],
  },
];

export const SOUNDS: Sound[] = SOUND_GROUPS.flatMap((g) => g.sounds);

/** A saved choice if it is still offered, else the grand piano. */
export function parseSound(value: unknown): number {
  return SOUNDS.some((s) => s.program === value) ? (value as number) : 0;
}
