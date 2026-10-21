// What the player is asked to play in Learn: the song's melody notes, its
// chords, or both, as targets on the song's beat grid.

import { QUALITY_INTERVALS } from '@midzer/audio-web';
import { parseChord } from '../songs/chords';
import { BEATS_PER_BAR, resolvedBeats, sectionsByBar, type Song } from '../songs/song';

export type Part = 'melody' | 'chords' | 'both';

export const PART_LABELS: Record<Part, string> = {
  melody: 'Melody',
  chords: 'Chords',
  both: 'Hands together',
};

export interface Target {
  /** Index in the target list. */
  id: number;
  kind: 'note' | 'chord';
  /** Start in song beats. */
  beat: number;
  /** Length in beats. */
  len: number;
  /** A note's key, or a chord voicing to show on the keyboard. */
  keys: number[];
  /** Pitch classes that must all sound; for a note, its own. */
  pcs: number[];
  /** The chord name, for chords. */
  name?: string;
}

export interface TargetOptions {
  /** Lowest key of the octave chord voicings are shown in. */
  chordBase?: number;
}

/** Parts a song can be practised with: chords always, melody when it has one. */
export function partsOf(song: Song): Part[] {
  return song.melody?.length ? ['melody', 'chords', 'both'] : ['chords'];
}

/** Targets sorted by beat. Chords are asked for where they change and again on each bar. */
export function buildTargets(song: Song, part: Part, { chordBase = 48 }: TargetOptions = {}): Target[] {
  const targets: Omit<Target, 'id'>[] = [];
  if (part !== 'chords') {
    for (const n of song.melody ?? []) {
      targets.push({ kind: 'note', beat: n.at, len: n.len, keys: [n.key], pcs: [n.key % 12] });
    }
  }
  if (part !== 'melody') targets.push(...chordTargets(song, chordBase));
  targets.sort((a, b) => a.beat - b.beat || (a.kind === b.kind ? 0 : a.kind === 'chord' ? -1 : 1));
  return targets.map((t, id) => ({ ...t, id }));
}

function chordTargets(song: Song, base: number): Omit<Target, 'id'>[] {
  const beats = resolvedBeats(song.bars);
  const sections = sectionsByBar(song.bars);
  const out: Omit<Target, 'id'>[] = [];
  let previous: string | null = null;
  beats.forEach((name, i) => {
    const bar = Math.floor(i / BEATS_PER_BAR);
    const changed = name !== previous;
    previous = name;
    // The intro is the band's alone.
    if (!name || sections[bar] === 'intro' || !(changed || i % BEATS_PER_BAR === 0)) return;
    const spec = parseChord(name);
    if (!spec) return;
    const intervals = QUALITY_INTERVALS[spec.quality] ?? [];
    const root = base + spec.root;
    // A four-note chord can leave out its fifth, as players often do.
    const required = intervals.filter((iv) => intervals.length < 4 || iv !== 7);
    const last = out[out.length - 1];
    if (last) last.len = i - last.beat;
    out.push({
      kind: 'chord',
      beat: i,
      len: BEATS_PER_BAR,
      keys: intervals.map((iv) => root + iv),
      pcs: required.map((iv) => (spec.root + iv) % 12),
      name,
    });
  });
  const last = out[out.length - 1];
  if (last) last.len = Math.min(last.len, beats.length - last.beat);
  return out;
}

/** The song beat after the last target ends. */
export function targetsEnd(targets: readonly Target[]): number {
  return targets.reduce((end, t) => Math.max(end, t.beat + t.len), 0);
}
