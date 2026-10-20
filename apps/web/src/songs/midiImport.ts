// MIDI file → song: reads the harmony of every half bar from the notes that
// sound in it and writes it as a chord chart.
//
// This is a different job from live chord recognition (chord.rs): a whole
// arrangement with melody and passing notes sounds at once, so each window is
// reduced to a duration-weighted pitch-class profile and matched against
// chord templates by cosine similarity, with a bonus when the lowest note is
// the root.

import { QUALITY_INTERVALS } from '@midzer/audio-web';
import { formatChord, Q } from './chords';
import { BEATS_PER_BAR, MAX_BARS, newSongId, type Bar, type Song } from './song';
import { readSmf, type Smf, type SmfNote } from './smf';

const DRUM_CHANNEL = 9;
/** Qualities worth telling apart in a full arrangement; rarer ones read as noise. */
const CANDIDATES = [Q.major, Q.minor, Q.dom7, Q.maj7, Q.min7, Q.dim, Q.sus4, Q.halfDim7];
const RARITY: Record<number, number> = { [Q.major]: 0, [Q.minor]: 0, [Q.dom7]: 0.04, [Q.maj7]: 0.05, [Q.min7]: 0.04 };
const BASS_BONUS = 0.08;
/** Below this similarity a window counts as no clear chord and holds the one before. */
const MIN_SCORE = 0.55;

export interface MidiImportResult {
  song: Song;
  /** Things the user should know, e.g. a meter the chart can't show. */
  warnings: string[];
}

export function importMidi(data: ArrayBuffer | Uint8Array, title: string): MidiImportResult {
  const smf = readSmf(data);
  const warnings: string[] = [];
  const notes = smf.tracks.flatMap((t) => t.notes).filter((n) => n.channel !== DRUM_CHANNEL && n.duration > 0);
  if (notes.length === 0) throw new Error('This MIDI file has no notes to read chords from.');

  const meter = smf.meters[0];
  if (meter && (meter.numerator !== 4 || meter.denominator !== 4)) {
    warnings.push(`The file is in ${meter.numerator}/${meter.denominator}; the chart is read as 4/4.`);
  }
  if (new Set(smf.tempos.map((t) => t.usPerQuarter)).size > 1) {
    warnings.push('The file changes tempo; the song uses the first tempo.');
  }

  const barTicks = smf.ppq * BEATS_PER_BAR;
  // Start at the bar of the first note, so silent lead-in bars are dropped.
  const firstBar = Math.floor(notes.reduce((min, n) => Math.min(min, n.tick), Infinity) / barTicks);
  const end = notes.reduce((max, n) => Math.max(max, n.tick + n.duration), 0);
  let barCount = Math.ceil(end / barTicks) - firstBar;
  if (barCount > MAX_BARS) {
    warnings.push(`Only the first ${MAX_BARS} bars were imported.`);
    barCount = MAX_BARS;
  }

  const half = barTicks / 2;
  const bars: Bar[] = [];
  let previous: string | null = null;
  for (let b = 0; b < barCount; b++) {
    const start = (firstBar + b) * barTicks;
    const halves = [chordIn(notes, start, start + half), chordIn(notes, start + half, start + barTicks)];
    const whole = chordIn(notes, start, start + barTicks);
    // One chord for the bar unless the halves clearly differ.
    const names = halves[0] && halves[1] && halves[0] !== halves[1] ? halves : [whole ?? halves[0] ?? halves[1], null];
    const beats = ['', '', '', ''];
    names.forEach((name, i) => {
      if (name && name !== previous) {
        beats[i * 2] = name;
        previous = name;
      }
    });
    bars.push({ beats });
  }

  const name = smf.tracks.find((t) => t.name)?.name;
  const song: Song = {
    id: newSongId(),
    title: title || name || 'Imported song',
    bpm: Math.round(Math.min(240, Math.max(40, bpmOf(smf)))),
    style: 0,
    bars,
    source: 'midi',
  };
  return { song, warnings };
}

function bpmOf(smf: Smf): number {
  const first = smf.tempos[0];
  return first ? 60_000_000 / first.usPerQuarter : 120;
}

/** The chord sounding in `[from, to)`, or null if nothing clear does. */
export function chordIn(notes: SmfNote[], from: number, to: number): string | null {
  const profile = new Array<number>(12).fill(0);
  let lowest = Infinity;
  let bassPc = -1;
  for (const n of notes) {
    const overlap = Math.min(to, n.tick + n.duration) - Math.max(from, n.tick);
    if (overlap <= 0) continue;
    profile[n.key % 12]! += overlap * (0.5 + n.velocity / 254);
    if (n.key < lowest) {
      lowest = n.key;
      bassPc = n.key % 12;
    }
  }
  const norm = Math.hypot(...profile);
  if (norm === 0) return null;

  let best: { score: number; root: number; quality: number } | null = null;
  for (let root = 0; root < 12; root++) {
    for (const quality of CANDIDATES) {
      const tones = QUALITY_INTERVALS[quality]!.map((i) => (root + i) % 12);
      const dot = tones.reduce((sum, pc) => sum + profile[pc]!, 0);
      const score =
        dot / (norm * Math.sqrt(tones.length)) - (RARITY[quality] ?? 0.08) + (bassPc === root ? BASS_BONUS : 0);
      if (!best || score > best.score) best = { score, root, quality };
    }
  }
  if (!best || best.score < MIN_SCORE) return null;
  const bass = tonesOf(best).includes(bassPc) ? bassPc : best.root;
  return formatChord({ root: best.root, quality: best.quality, bass });
}

function tonesOf({ root, quality }: { root: number; quality: number }): number[] {
  return QUALITY_INTERVALS[quality]!.map((i) => (root + i) % 12);
}
