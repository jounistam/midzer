// Built-in songs: common progressions (chord progressions are free to use)
// and traditional tunes in the public domain. Style indices follow STYLES.

import { songFromChart, type Song } from './song';

const POP = 0;
const ROCK = 1;
const FUNK = 2;
const BOSSA = 3;
const BALLAD = 4;
const BLUES = 5;

const song = (id: string, title: string, artist: string, bpm: number, style: number, chart: string): Song =>
  songFromChart({ id: `builtin-${id}`, title, artist, bpm, style, source: 'builtin' }, chart);

export const BUILTIN_SONGS: Song[] = [
  song(
    'four-chords',
    'Four Chord Anthem',
    'I–V–vi–IV in C',
    100,
    POP,
    `[Intro] C | G | Am | F |
     [Verse] C | G | Am | F | C | G | Am | [Fill] F |
     [Chorus] F | G | C | Am | F | G | C | [Fill] C |
     [Verse] C | G | Am | F | C | G | Am | [Fill] F |
     [Chorus] F | G | C | Am | F | G | C | C |
     [Ending] C`,
  ),
  song(
    'twelve-bar',
    'Twelve-Bar Blues in A',
    'Blues progression',
    96,
    BLUES,
    `[Intro] E7 |
     [A] A7 | D7 | A7 | A7 | D7 | D7 | A7 | A7 | E7 | D7 | A7 | [Fill] E7 |
     [B] A7 | D7 | A7 | A7 | D7 | D7 | A7 | A7 | E7 | D7 | A7 | [Fill] E7 |
     [C] A7 | D7 | A7 | A7 | D7 | D7 | A7 | A7 | E7 | D7 | A7 | E7 |
     [Ending] A7`,
  ),
  song(
    'saints',
    'When the Saints Go Marching In',
    'Traditional',
    112,
    POP,
    `[Intro] G7 |
     [A] C | C | C | C | C | C | G7 | G7 | C | C7 | F | F | C | G7 | C | [Fill] C |
     [B] C | C | C | C | C | C | G7 | G7 | C | C7 | F | F | C | G7 | C | C |
     [Ending] C`,
  ),
  song(
    'susanna',
    'Oh! Susanna',
    'Stephen Foster, 1848',
    120,
    POP,
    `[A] C | C | C | G7 | C | C | C G7 | C |
     C | C | C | G7 | C | C | C G7 | [Fill] C |
     [B] F | F | C | C G7 | C | C | C G7 | C |
     [Ending] C`,
  ),
  song(
    'canon',
    'Canon Progression',
    'After Pachelbel',
    72,
    BALLAD,
    `[Intro] D | A |
     [A] D A | Bm F#m | G D | G A | D A | Bm F#m | G D | G A |
     [B] D A | Bm F#m | G D | G A | D A | Bm F#m | G D | G A |
     [Ending] D`,
  ),
  song(
    'doo-wop',
    'Fifties Doo-Wop',
    'I–vi–IV–V in C',
    76,
    BALLAD,
    `[A] C | Am | F | G | C | Am | F | G |
     [B] C | Am | F | G | C | Am | F | G7 |
     [Ending] C`,
  ),
  song(
    'ii-v-i',
    'Two-Five-One',
    'Jazz cadences in C',
    140,
    BOSSA,
    `[A] Dm7 | G7 | Cmaj7 | Cmaj7 | Dm7 | G7 | Cmaj7 | A7 |
     Dm7 | G7 | Em7 A7 | Dm7 G7 |
     [B] Fmaj7 | Fm7 | Em7 | A7 | Dm7 | G7 | Cmaj7 | [Fill] Cmaj7 |
     [A] Dm7 | G7 | Cmaj7 | A7 | Dm7 | G7 | Cmaj7 | Cmaj7 |
     [Ending] Cmaj7`,
  ),
  song(
    'andalusian',
    'Andalusian Cadence',
    'Am–G–F–E',
    120,
    ROCK,
    `[Intro] Am | E |
     [A] Am G | F E | Am G | F E | Am G | F E | Am G | [Fill] F E |
     [B] Dm | Am | E | Am | Dm | Am | E | [Fill] E7 |
     [A] Am G | F E | Am G | F E |
     [Ending] Am`,
  ),
  song(
    'funk-vamp',
    'One-Chord Funk Vamp',
    'E minor groove',
    104,
    FUNK,
    `[A] Em7 | Em7 | Em7 | Em7 | Em7 | Em7 | Em7 | [Fill] Em7 |
     [B] A7 | A7 | Em7 | Em7 | A7 | A7 | Em7 | [Fill] Em7 |
     [C] Cmaj7 | B7 | Em7 | Em7 |
     [Ending] Em7`,
  ),
];
