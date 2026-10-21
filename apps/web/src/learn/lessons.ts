// The lesson path: short exercises and built-in songs in an order that builds
// up, from a first chord to playing melody and chords together. Each lesson
// opens once the one before it has a star.

import { STYLES } from '@midzer/audio-web';
import { BUILTIN_SONGS } from '../songs/builtin';
import { melody } from '../songs/melody';
import { songFromChart, type Song } from '../songs/song';
import type { Progress } from './progress';
import type { Part } from './targets';

export interface Lesson {
  id: string;
  unit: string;
  title: string;
  /** What the lesson teaches, on its card. */
  blurb: string;
  /** Shown above the falling notes while practising. */
  tip: string;
  song: Song;
  part: Part;
  /** Wait mode to start with; the player can change it. */
  wait: boolean;
  /** Tempo to start with, as a share of the song's. */
  speed: number;
}

const POP = STYLES.findIndex((s) => s.name === 'Pop');
const BALLAD = STYLES.findIndex((s) => s.name === 'Ballad');

const exercise = (id: string, title: string, bpm: number, style: number, chart: string, tune?: string): Song => {
  const song = songFromChart({ id: `lesson-${id}`, title, bpm, style, source: 'builtin' }, chart);
  return tune ? { ...song, melody: melody(tune) } : song;
};

const builtin = (id: string): Song => {
  const song = BUILTIN_SONGS.find((s) => s.id === `builtin-${id}`);
  if (!song) throw new Error(`No built-in song ${id}`);
  return song;
};

export const LESSONS: Lesson[] = [
  {
    id: 'first-chord',
    unit: 'Chords with the band',
    title: 'Your first chord: C',
    blurb: 'Three white keys, C E G. The band waits until you play them.',
    tip: 'Play C, E and G together each time a bar starts.',
    song: exercise('first-chord', 'Your first chord: C', 84, POP, '[Intro] C | [A] C | C | C | C | [Ending] C'),
    part: 'chords',
    wait: true,
    speed: 1,
  },
  {
    id: 'three-chords',
    unit: 'Chords with the band',
    title: 'C, F and G',
    blurb: 'The three chords behind thousands of songs.',
    tip: 'F is F A C, G is G B D. Move your hand, keep the shape.',
    song: exercise('three-chords', 'C, F and G', 84, POP, '[Intro] C | [A] C | F | G | C | C | F | G | C | [Ending] C'),
    part: 'chords',
    wait: true,
    speed: 1,
  },
  {
    id: 'minor',
    unit: 'Chords with the band',
    title: 'A minor and the four chords',
    blurb: 'Am is A C E: all white keys, and it sounds sad.',
    tip: 'C, G, Am, F: the four-chord song. Try it with wait mode off once it feels easy.',
    song: exercise('minor', 'A minor and the four chords', 88, POP, '[Intro] C | [A] C | G | Am | F | C | G | Am | F | [Ending] C'),
    part: 'chords',
    wait: true,
    speed: 1,
  },
  {
    id: 'anthem',
    unit: 'Chords with the band',
    title: 'Four Chord Anthem',
    blurb: 'A whole song in time with the band, chords changing every bar.',
    tip: 'Play each chord on the bar line. The band keeps going, so stay with it.',
    song: builtin('four-chords'),
    part: 'chords',
    wait: false,
    speed: 0.85,
  },
  {
    id: 'five-fingers',
    unit: 'First melodies',
    title: 'Five fingers',
    blurb: 'Thumb on C, one finger per key up to G.',
    tip: 'Right hand: thumb on C, then D E F G with the next fingers.',
    song: exercise(
      'five-fingers',
      'Five fingers',
      80,
      BALLAD,
      '[Intro] C | [A] C | G | C | G C | [Ending] C',
      'r:4 | C4 D E F | G F E D | C E G E | D:2 C:2',
    ),
    part: 'melody',
    wait: true,
    speed: 1,
  },
  {
    id: 'mary',
    unit: 'First melodies',
    title: 'Mary Had a Little Lamb',
    blurb: 'Three notes, E D C, and a G at the top.',
    tip: 'Start on E with your middle finger.',
    song: builtin('mary'),
    part: 'melody',
    wait: true,
    speed: 0.9,
  },
  {
    id: 'twinkle',
    unit: 'First melodies',
    title: 'Twinkle, Twinkle, Little Star',
    blurb: 'A jump from C up to G, then step by step back down.',
    tip: 'The jump C to G is your thumb to your little finger.',
    song: builtin('twinkle'),
    part: 'melody',
    wait: true,
    speed: 0.9,
  },
  {
    id: 'ode',
    unit: 'First melodies',
    title: 'Ode to Joy',
    blurb: 'Beethoven’s tune, in time with the band.',
    tip: 'Mostly steps. Watch the long notes at the end of each line.',
    song: builtin('ode-to-joy'),
    part: 'melody',
    wait: false,
    speed: 0.8,
  },
  {
    id: 'mary-together',
    unit: 'Hands together',
    title: 'Mary, hands together',
    blurb: 'Left hand chords, right hand melody.',
    tip: 'Left hand plays the chord on each bar line, right hand the tune.',
    song: builtin('mary'),
    part: 'both',
    wait: true,
    speed: 0.8,
  },
  {
    id: 'ode-together',
    unit: 'Hands together',
    title: 'Ode to Joy, hands together',
    blurb: 'The whole tune with chords, in time.',
    tip: 'Slow it down first. Speed it up when you get three stars.',
    song: builtin('ode-to-joy'),
    part: 'both',
    wait: false,
    speed: 0.7,
  },
];

/** Units in path order. */
export function units(lessons: readonly Lesson[] = LESSONS): { unit: string; lessons: Lesson[] }[] {
  const out: { unit: string; lessons: Lesson[] }[] = [];
  for (const lesson of lessons) {
    const last = out[out.length - 1];
    if (last?.unit === lesson.unit) last.lessons.push(lesson);
    else out.push({ unit: lesson.unit, lessons: [lesson] });
  }
  return out;
}

/** A lesson is open when it is the first one or the one before it has a star. */
export function isUnlocked(id: string, progress: ReadonlyMap<string, Progress>, lessons: readonly Lesson[] = LESSONS): boolean {
  const i = lessons.findIndex((l) => l.id === id);
  if (i <= 0) return i === 0;
  return (progress.get(lessons[i - 1]!.id)?.stars ?? 0) > 0;
}

/** The first open lesson without a star, to suggest next. */
export function nextLesson(progress: ReadonlyMap<string, Progress>, lessons: readonly Lesson[] = LESSONS): Lesson | null {
  return lessons.find((l) => isUnlocked(l.id, progress, lessons) && !(progress.get(l.id)?.stars ?? 0)) ?? null;
}

export function lessonAfter(id: string, lessons: readonly Lesson[] = LESSONS): Lesson | null {
  const i = lessons.findIndex((l) => l.id === id);
  return i >= 0 ? (lessons[i + 1] ?? null) : null;
}
